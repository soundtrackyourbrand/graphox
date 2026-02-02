use dashmap::DashMap;
use ropey::Rope;
use tower_lsp::{Client, LanguageServer, LspService, Server, jsonrpc::Result, lsp_types::*};
use tree_sitter::{InputEdit, Point, Query, QueryCursor, StreamingIterator, Tree};

struct DocumentState {
    uri: Url,
    rope: Rope,
    tree: Tree,
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

impl DocumentState {
    fn new(uri: Url, text: &str, mut parser: tree_sitter::Parser) -> Self {
        let rope = Rope::from_str(text);
        let tree = parser.parse(text, None).unwrap();
        Self { uri, rope, tree }
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
        self.tree.edit(&edit);

        // We feed the Rope chunks to Tree-sitter for maximum efficiency
        self.tree = parser
            .parse_with_options(
                &mut |byte, _| {
                    if byte >= self.rope.len_bytes() {
                        return "";
                    }
                    let (chunk, chunk_byte, _, _) = self.rope.chunk_at_byte(byte);
                    &chunk[byte - chunk_byte..]
                },
                Some(&self.tree),
                None,
            )
            .unwrap();
    }

    pub fn get_definition_location(&self, position: Position, uri: &Url) -> Option<Location> {
        // 1. Convert LSP position to Tree-sitter Point
        let point = tree_sitter::Point::new(position.line as usize, position.character as usize);

        // 2. Find the smallest node at that coordinate
        let root = self.tree.root_node();
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
            return self.find_definition_in_tree(&symbol_name, uri);
        }

        None
    }
    fn find_definition_in_tree(&self, target_name: &str, uri: &Url) -> Option<Location> {
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
        let text = self.rope.to_string();
        let mut matches = cursor.matches(&query, self.tree.root_node(), text.as_bytes());

        if let Some(m) = matches.next() {
            let node = m.captures[0].node;
            return Some(Location {
                uri: uri.clone(), // In a real LSP, you'd search all files in the DashMap
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
        let text = self.rope.to_string();
        let mut matches = cursor.matches(&query, self.tree.root_node(), text.as_bytes());

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
        let root = self.tree.root_node();
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
        let text = self.rope.to_string();
        let mut matches = cursor.matches(&query, self.tree.root_node(), text.as_bytes());

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
}

struct Backend {
    client: Client,
    // Use a Map to track multiple open files
    documents: DashMap<Url, DocumentState>,
}

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

            // 4. (Optional) Immediately calculate and publish new diagnostics
            // let diagnostics = self.get_diagnostics_for_doc(&doc);
            // self.client
            //     .publish_diagnostics(uri, diagnostics, None)
            //     .await;
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
            && let Some(location) = doc.get_definition_location(position, &uri)
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

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
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
