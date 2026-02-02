use std::sync::{Arc, RwLock};

use apollo_compiler::{Schema, schema};
use dashmap::DashMap;
use ropey::Rope;
use tower_lsp::{Client, LanguageServer, LspService, Server, jsonrpc::Result, lsp_types::*};
use tree_sitter::{InputEdit, Node, Parser, Point, Query, QueryCursor, StreamingIterator, Tree};

struct DocumentState {
    uri: Url,
    rope: Rope,
    ts_tree: Tree,
}

const GQL_SYMBOL_QUERY: &str = r#"
    (object_type_definition 
        name: (name) @symbol.name) @symbol.container

    (enum_type_definition 
        name: (name) @symbol.name) @symbol.container

    (fragment_definition 
        name: (name) @symbol.name) @symbol.container

    (interface_type_definition 
        name: (name) @symbol.name) @symbol.container
"#;

const SEMANTIC_TOKEN_QUERY: &str = r#"
    (name) @variable
    (type_name) @type
    (scalar_type) @keyword
    (enum_value) @enum
    (string) @string
"#;

// A query to find: gql` ... `
const TS_GQL_QUERY: &str = r#"
    (tagged_template_expression
        tag: (identifier) @tag_name
        template: (template_string) @gql_content
        (#eq? @tag_name "gql")
    )
"#;

fn mask_interpolations(text: &str) -> String {
    let mut masked = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            // We found a ${ ... }
            masked.push(' '); // Replace '$'
            masked.push(' '); // Replace '{'
            chars.next(); // Consume '{'

            let mut depth = 1;
            while depth > 0 {
                if let Some(inner_c) = chars.next() {
                    if inner_c == '{' {
                        depth += 1;
                    }
                    if inner_c == '}' {
                        depth -= 1;
                    }
                    masked.push(' '); // Mask everything inside with whitespace
                } else {
                    break;
                }
            }
        } else {
            masked.push(c);
        }
    }
    masked
}

impl DocumentState {
    fn new(uri: Url, text: &str, mut parser: tree_sitter::Parser) -> Self {
        let rope = Rope::from_str(text);
        let tree = parser.parse(text, None).unwrap();
        Self {
            uri,
            rope,
            ts_tree: tree,
        }
    }

    pub fn process_embedded_gql(&self) -> Vec<(Tree, usize)> {
        let ts_lang = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        let query = tree_sitter::Query::new(&ts_lang, TS_GQL_QUERY).unwrap();
        let mut cursor = tree_sitter::QueryCursor::new();

        let mut ts_matches = cursor.matches(&query, self.ts_tree.root_node(), |node: Node| {
            let start = node.start_byte();
            let end = node.end_byte();

            // We return a set of chunks from the rope for that specific node range
            // Since 'matches' expects an iterator of bytes, we can use Rope's chunks
            self.rope.byte_slice(start..end).chunks()
        });

        let mut gql_blocks = vec![];
        while let Some(m) = ts_matches.next() {
            let gql_node = m.captures[1].node;

            // Extract the range (excluding backticks)
            let start_byte = gql_node.start_byte() + 1;
            let end_byte = gql_node.end_byte() - 1;
            let raw_gql = self.rope.byte_slice(start_byte..end_byte).to_string();

            // 1. Mask the JS interpolations so GraphQL parser doesn't crash
            let masked_gql = mask_interpolations(&raw_gql);

            // 2. Parse the masked string
            let mut gql_parser = Parser::new();
            gql_parser
                .set_language(&tree_sitter_graphql::LANGUAGE.into())
                .unwrap();

            if let Some(gql_tree) = gql_parser.parse(&masked_gql, None) {
                // 3. Analyze the GQL tree
                // IMPORTANT: Pass the 'start_byte' so diagnostics know where
                // they are in the actual .ts file!
                // self.analyze_gql_content(gql_tree, start_byte);
                gql_blocks.push((gql_tree, start_byte));
            }
        }
        gql_blocks
    }

    fn translate_to_file_range(&self, gql_node: tree_sitter::Node, offset_byte: usize) -> Range {
        // Offset the byte position
        let absolute_start_byte = gql_node.start_byte() + offset_byte;
        let absolute_end_byte = gql_node.end_byte() + offset_byte;

        // Use Ropey to get Line/Col from the absolute byte offset
        let start_char = self.rope.byte_to_char(absolute_start_byte);
        let end_char = self.rope.byte_to_char(absolute_end_byte);

        Range {
            start: Position::new(
                self.rope.char_to_line(start_char) as u32,
                (start_char - self.rope.line_to_char(self.rope.char_to_line(start_char))) as u32,
            ),
            end: Position::new(
                self.rope.char_to_line(end_char) as u32,
                (end_char - self.rope.line_to_char(self.rope.char_to_line(end_char))) as u32,
            ),
        }
    }

    pub fn get_semantic_diagnostics(&self, schema: &Schema) -> Vec<Diagnostic> {
        let mut all_diagnostics = Vec::new();

        // 1. Find all gql`...` blocks (reusing our TS Tree-sitter query)
        let blocks = self.process_embedded_gql();

        for block in blocks {
            self.collect_gql_errors(block.0.root_node(), block.1, &mut all_diagnostics);
        }
        all_diagnostics
    }

    fn collect_gql_errors(
        &self,
        node: tree_sitter::Node,
        offset_byte: usize,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        if node.is_error() || node.is_missing() {
            // Translate the local GQL node position to the absolute file range
            let range = self.translate_to_file_range(node, offset_byte);

            diagnostics.push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::ERROR),
                message: format!("GraphQL Syntax Error: unexpected '{}'", node.kind()),
                ..Default::default()
            });
        }

        // Recursively check children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_gql_errors(child, offset_byte, diagnostics);
        }
    }

    fn apply_change(
        &mut self,
        change: &TextDocumentContentChangeEvent,
        parser: &mut tree_sitter::Parser,
    ) {
        let range = change.range.expect("Incremental updates require a range");

        // 1. Calculate Byte Offsets using Ropey
        // Ropey makes this O(log N) instead of O(N)
        let start_char =
            self.rope.line_to_char(range.start.line as usize) + range.start.character as usize;
        // let start_char = self.rope.utf16_cu_to_char(
        //     self.rope.line_to_utf16_cu(range.start.line as usize) + range.start.character as usize,
        // );
        let end_char =
            self.rope.line_to_char(range.end.line as usize) + range.end.character as usize;

        let start_byte = self.rope.char_to_byte(start_char);
        let old_end_byte = self.rope.char_to_byte(end_char);

        // 2. Update the Rope (the actual text)
        // We remove the old range and insert the new text
        self.rope.remove(start_char..end_char);
        self.rope.insert(start_char, &change.text);

        // 3. Calculate the new end byte and position
        let new_end_byte = start_byte + change.text.len();
        let new_end_char = start_char + change.text.chars().count();
        let new_end_line = self.rope.char_to_line(new_end_char);
        let new_end_col = new_end_char - self.rope.line_to_char(new_end_line);

        // 4. Create the Tree-sitter Edit
        let edit = InputEdit {
            start_byte,
            old_end_byte,
            new_end_byte,
            start_position: Point::new(range.start.line as usize, range.start.character as usize),
            old_end_position: Point::new(range.end.line as usize, range.end.character as usize),
            new_end_position: Point::new(new_end_line, new_end_col),
        };

        // 5. Update the Tree
        self.ts_tree.edit(&edit);

        // We feed the Rope chunks to Tree-sitter for maximum efficiency
        self.ts_tree = parser
            .parse_with_options(
                &mut |byte, _| {
                    if byte >= self.rope.len_bytes() {
                        return "";
                    }
                    let (chunk, chunk_byte, _, _) = self.rope.chunk_at_byte(byte);
                    &chunk[byte - chunk_byte..]
                },
                Some(&self.ts_tree),
                None,
            )
            .unwrap();
    }

    pub fn get_definition_location(&self, position: Position) -> Option<Location> {
        // 1. Convert LSP position to Tree-sitter Point
        let point = tree_sitter::Point::new(position.line as usize, position.character as usize);

        // 2. Find the smallest node at that coordinate
        let root = self.ts_tree.root_node();
        let trigger_node = root.descendant_for_point_range(point, point)?;

        // 3. If we clicked a name, let's find where that name is defined
        if trigger_node.kind() == "name" {
            let symbol_name = self
                .rope
                .slice(
                    self.rope.byte_to_char(trigger_node.start_byte())
                        ..self.rope.byte_to_char(trigger_node.end_byte()),
                )
                .to_string();

            // 4. Search the tree for a definition with this name
            return self.find_definition_in_tree(&symbol_name);
        }

        None
    }
    fn find_definition_in_tree(&self, target_name: &str) -> Option<Location> {
        let lang = tree_sitter_graphql::LANGUAGE.into();
        // This query looks for any top-level definition with a matching name
        let query_str = format!(
            r#"(
                [
                    (object_type_definition name: (name) @name)
                    (fragment_definition name: (name) @name)
                    (enum_type_definition name: (name) @name)
                ]
                (#eq? @name "{}")
            )"#,
            target_name
        );

        let query = tree_sitter::Query::new(&lang, &query_str).ok()?;
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut matches = cursor.matches(&query, self.ts_tree.root_node(), |node: Node| {
            let start = node.start_byte();
            let end = node.end_byte();

            // We return a set of chunks from the rope for that specific node range
            // Since 'matches' expects an iterator of bytes, we can use Rope's chunks
            self.rope.byte_slice(start..end).chunks()
        });

        if let Some(m) = matches.next() {
            let node = m.captures[0].node;
            return Some(Location {
                uri: self.uri.clone(), // In a real LSP, you'd search all files in the DashMap
                range: node_to_lsp_range(node),
            });
        }
        None
    }

    pub fn get_symbols(&self) -> Vec<DocumentSymbol> {
        let lang = tree_sitter_graphql::LANGUAGE.into();
        let query = Query::new(&lang, GQL_SYMBOL_QUERY).unwrap();
        let mut cursor = QueryCursor::new();

        // Execute query on the root node
        let mut matches = cursor.matches(&query, self.ts_tree.root_node(), |node: Node| {
            let start = node.start_byte();
            let end = node.end_byte();

            // We return a set of chunks from the rope for that specific node range
            // Since 'matches' expects an iterator of bytes, we can use Rope's chunks
            self.rope.byte_slice(start..end).chunks()
        });

        let mut symbols = Vec::new();

        while let Some(m) = matches.next() {
            // In our query, @symbol.name is the first capture (index 0)
            let name_node = m.captures[0].node;
            let container_node = m.captures[1].node;

            let name = self
                .rope
                .slice(
                    self.rope.byte_to_char(name_node.start_byte())
                        ..self.rope.byte_to_char(name_node.end_byte()),
                )
                .to_string();

            #[allow(deprecated)]
            symbols.push(DocumentSymbol {
                name,
                detail: Some(format!("GraphQL {}", container_node.kind())),
                kind: SymbolKind::STRUCT, // Map GQL types to LSP kinds
                tags: None,
                deprecated: None,
                range: node_to_lsp_range(container_node),
                selection_range: node_to_lsp_range(name_node),
                children: None,
            });
        }
        symbols
    }

    pub fn get_hover_info(&self, position: Position) -> Option<Hover> {
        let point = Point::new(position.line as usize, position.character as usize);
        let root = self.ts_tree.root_node();
        let node = root.descendant_for_point_range(point, point)?;

        // Only trigger hover on "name" nodes
        if node.kind() == "name" {
            let symbol_name = self
                .rope
                .slice(
                    self.rope.byte_to_char(node.start_byte())
                        ..self.rope.byte_to_char(node.end_byte()),
                )
                .to_string();

            // Find the definition and its description
            if let Some(description) = self.find_description(&symbol_name) {
                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: format!("### {}\n---\n{}", symbol_name, description),
                    }),
                    range: Some(node_to_lsp_range(node)),
                });
            }
        }
        None
    }

    fn find_description(&self, target_name: &str) -> Option<String> {
        let lang = tree_sitter_graphql::LANGUAGE.into();
        // GraphQL allows descriptions as strings before definitions
        let query_str = format!(
            r#"(
                [
                    (object_type_definition description: (string)? @desc name: (name) @name)
                    (enum_type_definition description: (string)? @desc name: (name) @name)
                ]
                (#eq? @name "{}")
            )"#,
            target_name
        );

        let query = tree_sitter::Query::new(&lang, &query_str).ok()?;
        let mut cursor = tree_sitter::QueryCursor::new();
        let mut matches = cursor.matches(&query, self.ts_tree.root_node(), |node: Node| {
            let start = node.start_byte();
            let end = node.end_byte();

            // We return a set of chunks from the rope for that specific node range
            // Since 'matches' expects an iterator of bytes, we can use Rope's chunks
            self.rope.byte_slice(start..end).chunks()
        });

        while let Some(m) = matches.next() {
            // Check if we captured a description (@desc)
            if let Some(desc_node) = m.nodes_for_capture_index(0).next() {
                return Some(
                    self.rope
                        .slice(
                            self.rope.byte_to_char(desc_node.start_byte())
                                ..self.rope.byte_to_char(desc_node.end_byte()),
                        )
                        .to_string()
                        .trim_matches('"')
                        .to_string(),
                );
            }
        }
        None
    }

    pub fn get_semantic_tokens(&self) -> Vec<SemanticToken> {
        let lang = tree_sitter_graphql::LANGUAGE.into();
        let query = Query::new(&lang, SEMANTIC_TOKEN_QUERY).unwrap();
        let mut cursor = QueryCursor::new();

        let mut matches = cursor.matches(&query, self.ts_tree.root_node(), |node: Node| {
            let start = node.start_byte();
            let end = node.end_byte();

            // We return a set of chunks from the rope for that specific node range
            // Since 'matches' expects an iterator of bytes, we can use Rope's chunks
            self.rope.byte_slice(start..end).chunks()
        });

        let mut tokens = Vec::new();

        while let Some(m) = matches.next() {
            for cap in m.captures {
                let token_type = &query.capture_names()[cap.index as usize];
                tokens.push(node_to_semantic_token(
                    cap.node,
                    token_type_to_legend_index(token_type),
                ));
            }
        }

        tokens
    }
}

fn token_type_to_legend_index(token_type: &str) -> u32 {
    match token_type {
        "variable" => 0,
        "type" => 1,
        "keyword" => 2,
        "enum" => 3,
        "string" => 4,
        _ => 0,
    }
}

fn node_to_semantic_token(node: tree_sitter::Node, token_type: u32) -> SemanticToken {
    let start = node.start_position();
    let end = node.end_position();
    SemanticToken {
        delta_line: (start.row as u32),
        delta_start: (start.column as u32),
        length: ((end.column - start.column) as u32),
        token_type,
        token_modifiers_bitset: 0,
    }
}

struct Backend {
    client: Client,
    // Use a Map to track multiple open files
    documents: DashMap<Url, DocumentState>,
    schema: Arc<RwLock<Schema>>,
}

impl Backend {
    pub fn new(client: Client, schema_path: &str) -> Self {
        // Load the schema from a file on disk
        let schema_text = std::fs::read_to_string(schema_path).unwrap_or_else(|_| "".to_string());

        let schema =
            Schema::parse(&schema_text, schema_path).expect("Failed to parse initial schema");

        Self {
            client,
            documents: DashMap::new(),
            schema: Arc::new(RwLock::new(schema)), // Wrap it here
        }
    }

    async fn on_schema_file_changed(&self, new_text: &str) {
        let new_schema = Schema::parse(new_text, "schema.graphql").unwrap();
        let mut lock = self.schema.write().unwrap();
        *lock = new_schema;

        self.client
            .log_message(MessageType::INFO, "Schema updated!")
            .await;
    }

    async fn reload_schema(&self, path: &str) {
        if let Ok(text) = std::fs::read_to_string(path) {
            match Schema::parse(&text, path) {
                Ok(new_schema) => {
                    // 3. Acquire the write lock and update the shared schema
                    {
                        let mut lock = self.schema.write().unwrap();
                        *lock = new_schema;
                    }
                    self.client
                        .log_message(MessageType::INFO, "Schema successfully reloaded!")
                        .await;
                }
                Err(e) => {
                    self.client
                        .show_message(MessageType::ERROR, format!("Schema parse error: {}", e))
                        .await;
                }
            }
        }
    }
}

const SEMANTIC_TOKEN_LEGEND: &[SemanticTokenType] = &[
    SemanticTokenType::VARIABLE,
    SemanticTokenType::TYPE,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::ENUM,
    SemanticTokenType::STRING,
];

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    // 1. Tell the client what features we support
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::INCREMENTAL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            work_done_progress_options: WorkDoneProgressOptions {
                                work_done_progress: None,
                            },
                            legend: SemanticTokensLegend {
                                token_types: SEMANTIC_TOKEN_LEGEND.to_vec(),
                                token_modifiers: vec![],
                            },
                            range: Some(false),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                        },
                    ),
                ),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".to_string()]),
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "LSP Started!")
            .await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(doc) = self.documents.get(uri) {
            return Ok(doc.get_hover_info(position));
        }

        Ok(None)
    }

    async fn completion(&self, _: CompletionParams) -> Result<Option<CompletionResponse>> {
        let completions = vec![
            CompletionItem::new_simple("Hello".to_string(), "The greeting of kings".to_string()),
            CompletionItem::new_simple("World".to_string(), "The place we live".to_string()),
        ];
        Ok(Some(CompletionResponse::Array(completions)))
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;

        // 1. Get a mutable reference to the document from the DashMap
        if let Some(mut doc) = self.documents.get_mut(&uri) {
            // 2. We need a parser to handle the incremental re-parsing
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&tree_sitter_graphql::LANGUAGE.into())
                .unwrap();

            // 3. Apply every change in the batch (VS Code often batches keystrokes)
            for change in params.content_changes {
                doc.apply_change(&change, &mut parser);
            }

            // Run validation
            let diagnostics = {
                let schema = self.schema.read().unwrap();
                doc.get_semantic_diagnostics(&schema)
            };

            self.client
                .publish_diagnostics(uri, diagnostics, None)
                .await;
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        // Look up the document in our DashMap
        if let Some(doc) = self.documents.get(&uri)
            && let Some(location) = doc.get_definition_location(position)
        {
            return Ok(Some(GotoDefinitionResponse::Scalar(location)));
        }

        Ok(None)
    }

    // Don't forget to handle the initial "Open" event!
    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_graphql::LANGUAGE.into())
            .unwrap();

        let doc = DocumentState::new(
            params.text_document.uri.clone(),
            &params.text_document.text,
            parser,
        );
        self.documents.insert(params.text_document.uri, doc);
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        if let Some(doc) = self.documents.get(&params.text_document.uri) {
            let symbols = doc.get_symbols();
            return Ok(Some(DocumentSymbolResponse::Nested(symbols)));
        }

        Ok(None)
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        if let Some(doc) = self.documents.get(&params.text_document.uri) {
            let tokens = doc.get_semantic_tokens();
            return Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
                result_id: None,
                data: tokens,
            })));
        }

        Ok(None)
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_change_configuration(&self, _: DidChangeConfigurationParams) {
        self.client
            .log_message(MessageType::INFO, "Configuration changed!")
            .await;

        // 1. Request the specific settings from VS Code
        // Use a helper struct to deserialize the JSON response
        let config = self
            .client
            .configuration(vec![ConfigurationItem {
                scope_uri: None,
                section: Some("gqlLsp.schemaPath".to_string()),
            }])
            .await;

        if let Ok(values) = config {
            if let Some(path_value) = values.first().and_then(|v| v.as_str()) {
                self.client
                    .log_message(
                        MessageType::INFO,
                        format!("New schema path: {}", path_value),
                    )
                    .await;

                // 2. Reload the schema from the new path
                self.reload_schema(path_value).await;
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let schema_path = "";
    let (service, socket) = LspService::new(|client| Backend::new(client, schema_path));
    Server::new(stdin, stdout, socket).serve(service).await;
}

// pub struct GraphQLAnalyzer {
//     parser: Parser,
// }
//
// impl GraphQLAnalyzer {
//     pub fn new() -> Self {
//         let mut parser = Parser::new();
//         parser
//             .set_language(&LANGUAGE.into())
//             .expect("Error loading GraphQL grammar");
//         Self { parser }
//     }
//
//     pub fn get_diagnostics(&mut self, text: &str) -> Vec<Diagnostic> {
//         let tree = self.parser.parse(text, None).unwrap();
//         let mut diagnostics = Vec::new();
//
//         // Start walking the tree from the root
//         self.find_errors(tree.root_node(), &mut diagnostics);
//         diagnostics
//     }
//
//     fn find_errors(&self, node: Node, diagnostics: &mut Vec<Diagnostic>) {
//         // Tree-sitter identifies syntax errors as ERROR or MISSING nodes
//         if node.is_error() || node.is_missing() {
//             diagnostics.push(Diagnostic {
//                 range: Range {
//                     // Tree-sitter positions are 0-indexed, matching LSP exactly
//                     start: Position::new(
//                         node.start_position().row as u32,
//                         node.start_position().column as u32,
//                     ),
//                     end: Position::new(
//                         node.end_position().row as u32,
//                         node.end_position().column as u32,
//                     ),
//                 },
//                 severity: Some(DiagnosticSeverity::ERROR),
//                 message: format!("Syntax error: unexpected '{}'", node.kind()),
//                 ..Default::default()
//             });
//         }
//
//         // Recursively check children
//         let mut cursor = node.walk();
//         for child in node.children(&mut cursor) {
//             self.find_errors(child, diagnostics);
//         }
//     }
// }

// Helper to convert Tree-sitter Node range to LSP Range
fn node_to_lsp_range(node: tree_sitter::Node) -> Range {
    Range {
        start: Position::new(
            node.start_position().row as u32,
            node.start_position().column as u32,
        ),
        end: Position::new(
            node.end_position().row as u32,
            node.end_position().column as u32,
        ),
    }
}

/*

use dashmap::DashMap;
use ropey::Rope;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};
use tree_sitter::{InputEdit, Parser, Point, Tree};

// --- DOCUMENT STATE ---

struct Document {
    uri: Url,
    rope: Rope,
    tree: Tree,
}

impl Document {
    fn new(uri: Url, text: &str, parser: &mut Parser) -> Self {
        let rope = Rope::from_str(text);
        let tree = parser.parse(text, None).expect("Initial parse failed");
        Self { uri, rope, tree }
    }

    fn apply_change(&mut self, change: &TextDocumentContentChangeEvent, parser: &mut Parser) {
        if let Some(range) = change.range {
            // 1. Calculate offsets (UTF-16 aware for LSP/VS Code compatibility)
            let start_char = self.rope.line_to_char(range.start.line as usize) + range.start.character as usize;
            let end_char = self.rope.line_to_char(range.end.line as usize) + range.end.character as usize;

            let start_byte = self.rope.char_to_byte(start_char);
            let old_end_byte = self.rope.char_to_byte(end_char);

            // 2. Update the Rope
            self.rope.remove(start_char..end_char);
            self.rope.insert(start_char, &change.text);

            // 3. Prepare Tree-sitter Edit
            let new_end_byte = start_byte + change.text.len();
            let new_end_char = start_char + change.text.chars().count();
            let new_end_line = self.rope.char_to_line(new_end_char);
            let new_end_col = new_end_char - self.rope.line_to_char(new_end_line);

            let edit = InputEdit {
                start_byte,
                old_end_byte,
                new_end_byte,
                start_position: Point::new(range.start.line as usize, range.start.character as usize),
                old_end_position: Point::new(range.end.line as usize, range.end.character as usize),
                new_end_position: Point::new(new_end_line, new_end_col),
            };

            // 4. Incremental Re-parse
            self.tree.edit(&edit);
            self.tree = parser.parse_with(&mut |byte, _| {
                if byte >= self.rope.len_bytes() { return ""; }
                let (chunk, chunk_byte, _, _) = self.rope.chunk_at_byte(byte);
                &chunk[byte - chunk_byte..]
            }, Some(&self.tree)).unwrap();
        }
    }
}

// --- LSP BACKEND ---

struct Backend {
    client: Client,
    documents: DashMap<Url, Document>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::INCREMENTAL)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let mut parser = create_parser();
        let doc = Document::new(params.text_document.uri.clone(), &params.text_document.text, &mut parser);
        self.documents.insert(params.text_document.uri, doc);
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let mut parser = create_parser();
        if let Some(mut doc) = self.documents.get_mut(&params.text_document.uri) {
            for change in params.content_changes {
                doc.apply_change(&change, &mut parser);
            }
        }
    }

    async fn document_symbol(&self, params: DocumentSymbolParams) -> Result<Option<DocumentSymbolResponse>> {
        if let Some(doc) = self.documents.get(&params.text_document.uri) {
            // Implementation would use Tree-sitter Queries here
            return Ok(Some(DocumentSymbolResponse::Flat(vec![])));
        }
        Ok(None)
    }

    async fn shutdown(&self) -> Result<()> { Ok(()) }
}

fn create_parser() -> Parser {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_graphql::LANGUAGE.into()).unwrap();
    parser
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend {
        client,
        documents: DashMap::new(),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}

*/
