use std::sync::{Arc, OnceLock, RwLock};

use apollo_compiler::{Schema, schema};
use dashmap::DashMap;
use ropey::Rope;
use tower_lsp::{Client, LanguageServer, jsonrpc::Result, lsp_types::*};
use tree_sitter::{InputEdit, Node, Parser, Point, Query, QueryCursor, StreamingIterator, Tree};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentLanguage {
    GraphQL,
    TypeScript,
}

impl DocumentLanguage {
    pub fn from_uri(uri: &Url) -> Self {
        let path = uri.path();
        if path.ends_with(".ts")
            || path.ends_with(".tsx")
            || path.ends_with(".cts")
            || path.ends_with(".mts")
            || path.ends_with(".js")
            || path.ends_with(".jsx")
            || path.ends_with(".cjs")
            || path.ends_with(".mjs")
        {
            DocumentLanguage::TypeScript
        } else {
            DocumentLanguage::GraphQL
        }
    }

    pub fn get_parser_language(&self) -> tree_sitter::Language {
        match self {
            DocumentLanguage::GraphQL => tree_sitter_graphql::LANGUAGE.into(),
            DocumentLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        }
    }
}

pub struct DocumentState {
    pub uri: Url,
    pub rope: Rope,
    pub tree: Tree,
    pub language: DocumentLanguage,
    pub graphql_trees: Vec<(Tree, usize)>,
}

static TS_QUERY_CACHE: OnceLock<Query> = OnceLock::new();
static GQL_SYMBOL_QUERY_CACHE: OnceLock<Query> = OnceLock::new();
static GQL_SEMANTIC_TOKEN_QUERY_CACHE: OnceLock<Query> = OnceLock::new();
static GQL_DEFINITION_QUERY_CACHE: OnceLock<Query> = OnceLock::new();
static GQL_DESCRIPTION_QUERY_CACHE: OnceLock<Query> = OnceLock::new();
static GQL_VALIDATION_QUERY_CACHE: OnceLock<Query> = OnceLock::new();

const GQL_SYMBOL_QUERY: &str = r#"
    (object_type_definition 
        (name) @symbol.name) @symbol.container

    (enum_type_definition 
        (name) @symbol.name) @symbol.container

    (fragment_definition 
        (name) @symbol.name) @symbol.container

    (interface_type_definition 
        (name) @symbol.name) @symbol.container
"#;

const SEMANTIC_TOKEN_QUERY: &str = r#"
    (name) @variable
    (named_type) @type
    (string_value) @string
"#;

// A query to find: gql` ... `
const TS_GQL_QUERY: &str = r#"
    (call_expression
        function: (identifier) @tag_name
        arguments: (template_string) @gql_content
        (#eq? @tag_name "gql")
    )
"#;

const GQL_DEFINITION_QUERY: &str = r#"
    (object_type_definition (name) @name)
    (fragment_definition (fragment_name (name) @name))
    (enum_type_definition (name) @name)
"#;

const GQL_DESCRIPTION_QUERY: &str = r#"
    (object_type_definition (description (string_value))? @desc (name) @name)
    (enum_type_definition (description (string_value))? @desc (name) @name)
"#;

const GQL_VALIDATION_QUERY: &str = r#"
    (operation_definition) @operation
    (fragment_definition) @fragment
    (type_condition) @type_cond
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
    pub fn new(uri: Url, text: &str, mut parser: tree_sitter::Parser) -> Self {
        let language = DocumentLanguage::from_uri(&uri);
        let rope = Rope::from_str(text);
        let tree = parser.parse(text, None).unwrap();
        let mut doc = Self {
            uri,
            rope,
            tree,
            language,
            graphql_trees: Vec::new(),
        };
        doc.graphql_trees = doc.reparse_graphql_trees();
        doc
    }

    fn reparse_graphql_trees(&self) -> Vec<(Tree, usize)> {
        if self.language == DocumentLanguage::GraphQL {
            return vec![(self.tree.clone(), 0)];
        }

        // TypeScript handling
        let query = TS_QUERY_CACHE.get_or_init(|| {
            let ts_lang = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
            tree_sitter::Query::new(&ts_lang, TS_GQL_QUERY).unwrap()
        });

        let mut cursor = tree_sitter::QueryCursor::new();

        let mut ts_matches = cursor.matches(query, self.tree.root_node(), |node: Node| {
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

    pub fn get_graphql_trees(&self) -> &Vec<(Tree, usize)> {
        &self.graphql_trees
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

    pub fn get_completion_items(&self, position: Position, schema: &Schema) -> Vec<CompletionItem> {
        let char_idx = self.rope.line_to_char(position.line as usize) + position.character as usize;
        let byte_offset = self.rope.char_to_byte(char_idx);

        for (tree, offset) in self.get_graphql_trees() {
            let offset = *offset;
            let root = tree.root_node();
            let tree_len = root.end_byte();

            if byte_offset >= offset && byte_offset <= offset + tree_len {
                // Find context by traversing
                if let Some(items) =
                    self.find_completions_in_tree(root, offset, byte_offset, schema)
                {
                    return items;
                }
            }
        }
        Vec::new()
    }

    fn find_completions_in_tree(
        &self,
        root: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
    ) -> Option<Vec<CompletionItem>> {
        let query = GQL_VALIDATION_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_VALIDATION_QUERY).unwrap()
        });

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, root, |n: Node| {
            let start = n.start_byte();
            let end = n.end_byte();
            self.rope
                .byte_slice((start + offset)..(end + offset))
                .chunks()
        });

        while let Some(m) = matches.next() {
            for cap in m.captures {
                let range = (cap.node.start_byte() + offset)..(cap.node.end_byte() + offset);

                println!(
                    "DEBUG: checking query match {} range {:?} vs cursor {}",
                    cap.node.kind(),
                    range,
                    cursor_offset
                );

                if cursor_offset >= range.start && cursor_offset <= range.end {
                    let capture_name = query.capture_names()[cap.index as usize];
                    if capture_name == "operation" {
                        if let Some(items) =
                            self.complete_operation(cap.node, offset, cursor_offset, schema)
                        {
                            return Some(items);
                        }
                    } else if capture_name == "fragment" {
                        if let Some(items) =
                            self.complete_fragment(cap.node, offset, cursor_offset, schema)
                        {
                            return Some(items);
                        }
                    } else if capture_name == "type_cond" {
                        return Some(self.get_all_type_completions(schema));
                    }
                }
            }
        }
        None
    }

    fn complete_operation(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
    ) -> Option<Vec<CompletionItem>> {
        println!("DEBUG: complete_operation");
        let mut operation_type_string = String::from("query");
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "operation_type" {
                operation_type_string = self.get_node_text(child, offset);
                break;
            }
        }

        let op_type = match operation_type_string.as_str() {
            "query" => Some(apollo_compiler::ast::OperationType::Query),
            "mutation" => Some(apollo_compiler::ast::OperationType::Mutation),
            "subscription" => Some(apollo_compiler::ast::OperationType::Subscription),
            _ => None,
        };

        if let Some(op) = op_type {
            let root_def = schema.root_operation(op);
            if let Some(root_def_name) = root_def {
                if let Some(root_type) = schema.types.get(root_def_name.as_str()) {
                    return self.complete_selection_set_recursive(
                        node,
                        offset,
                        cursor_offset,
                        root_type,
                        schema,
                    );
                }
            }
        }
        None
    }

    fn complete_fragment(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
    ) -> Option<Vec<CompletionItem>> {
        println!("DEBUG: complete_fragment");
        // Check if cursor is in type condition
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_condition" {
                let range = (child.start_byte() + offset)..(child.end_byte() + offset);
                if cursor_offset >= range.start && cursor_offset <= range.end {
                    // Return all types
                    return Some(self.get_all_type_completions(schema));
                }
            } else if child.kind() == "selection_set" {
                let range = (child.start_byte() + offset)..(child.end_byte() + offset);
                if cursor_offset >= range.start && cursor_offset <= range.end {
                    // We need the type of the fragment
                    // Find type condition again?
                    // Or traverse first to find type name.
                    if let Some(type_name) = self.get_fragment_type_condition(node, offset) {
                        if let Some(type_def) = schema.types.get(type_name.as_str()) {
                            return self.complete_selection_set_recursive(
                                child,
                                offset,
                                cursor_offset,
                                type_def,
                                schema,
                            );
                        }
                    }
                }
            }
        }
        None
    }

    fn get_fragment_type_condition(&self, node: Node, offset: usize) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_condition" {
                let mut tc_cursor = child.walk();
                for tc_child in child.children(&mut tc_cursor) {
                    if tc_child.kind() == "named_type" {
                        let mut nt_cursor = tc_child.walk();
                        for nt_child in tc_child.children(&mut nt_cursor) {
                            if nt_child.kind() == "name" {
                                return Some(self.get_node_text(nt_child, offset));
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn complete_selection_set_recursive(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
    ) -> Option<Vec<CompletionItem>> {
        println!(
            "DEBUG: complete_selection_set_recursive on type {:?} cursor {}",
            parent_type.name(),
            cursor_offset
        );
        let target_node = if node.kind() == "selection_set" {
            node
        } else {
            let mut found = None;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "selection_set" {
                    found = Some(child);
                    break;
                }
            }
            if found.is_none() {
                println!("DEBUG: selection_set not found in node {}", node.kind());
                return None;
            }
            found.unwrap()
        };

        let range = (target_node.start_byte() + offset)..(target_node.end_byte() + offset);
        println!("DEBUG: target_node range {:?}", range);
        if cursor_offset < range.start || cursor_offset > range.end {
            return None;
        }

        let mut cursor = target_node.walk();
        for child in target_node.children(&mut cursor) {
            let child_range = (child.start_byte() + offset)..(child.end_byte() + offset);
            println!(
                "DEBUG: checking child {} range {:?}",
                child.kind(),
                child_range
            );
            if cursor_offset >= child_range.start && cursor_offset <= child_range.end {
                let kind = child.kind();
                if kind == "field" || kind == "selection" {
                    // Handle selection wrapper
                    // Need to unwrap selection if present
                    let field_node = if kind == "selection" {
                        // find field child
                        let mut inner = child.walk();
                        if let Some(f) = child.children(&mut inner).find(|c| c.kind() == "field") {
                            f
                        } else {
                            continue; // No field inside selection (e.g. inline fragment?)
                        }
                    } else {
                        child
                    };

                    // Find name of field
                    let mut field_name_node = None;
                    let mut cursor_inner = field_node.walk();
                    for child in field_node.children(&mut cursor_inner) {
                        if child.kind() == "name" {
                            field_name_node = Some(child);
                            break;
                        }
                    }

                    if let Some(field_name_node) = field_name_node {
                        let field_name = self.get_node_text(field_name_node, offset);
                        println!("DEBUG: processing field '{}'", field_name);

                        // Look up field definition
                        let field_def = match parent_type {
                            schema::ExtendedType::Object(obj) => {
                                obj.fields.get(field_name.as_str())
                            }
                            schema::ExtendedType::Interface(iface) => {
                                iface.fields.get(field_name.as_str())
                            }
                            _ => None,
                        };

                        if let Some(field_def) = field_def {
                            println!("DEBUG: field def found");
                            // Recurse into sub-selection if cursor is deep
                            let mut sub_sel_set = None;
                            let mut f_cursor = field_node.walk();
                            for f_child in field_node.children(&mut f_cursor) {
                                if f_child.kind() == "selection_set" {
                                    sub_sel_set = Some(f_child);
                                    break;
                                }
                            }

                            if let Some(sss) = sub_sel_set {
                                let sss_range =
                                    (sss.start_byte() + offset)..(sss.end_byte() + offset);
                                println!(
                                    "DEBUG: checking sub-selection range {:?} vs cursor {}",
                                    sss_range, cursor_offset
                                );
                                if cursor_offset >= sss_range.start
                                    && cursor_offset <= sss_range.end
                                {
                                    // Recurse
                                    let field_type_name = field_def.ty.inner_named_type();
                                    if let Some(field_type_def) =
                                        schema.types.get(field_type_name.as_str())
                                    {
                                        return self.complete_selection_set_recursive(
                                            sss,
                                            offset,
                                            cursor_offset,
                                            field_type_def,
                                            schema,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // If we are here, we are in the selection set, but NOT deep inside a child's selection set.
        // (Or we are on a child name).
        // So return fields of `parent_type`.
        println!(
            "DEBUG: returning completions for type {:?}",
            parent_type.name()
        );
        Some(self.get_field_completions(parent_type))
    }

    fn get_field_completions(&self, parent_type: &schema::ExtendedType) -> Vec<CompletionItem> {
        let mut items = Vec::new();
        match parent_type {
            schema::ExtendedType::Object(obj) => {
                for (name, def) in &obj.fields {
                    items.push(CompletionItem {
                        label: name.to_string(),
                        kind: Some(CompletionItemKind::FIELD),
                        detail: Some(def.ty.to_string()),
                        ..Default::default()
                    });
                }
            }
            schema::ExtendedType::Interface(iface) => {
                for (name, def) in &iface.fields {
                    items.push(CompletionItem {
                        label: name.to_string(),
                        kind: Some(CompletionItemKind::FIELD),
                        detail: Some(def.ty.to_string()),
                        ..Default::default()
                    });
                }
            }
            // Union: __typename
            _ => {}
        }
        items.push(CompletionItem {
            label: "__typename".to_string(),
            kind: Some(CompletionItemKind::FIELD),
            detail: Some("String!".to_string()),
            ..Default::default()
        });
        items
    }

    fn get_all_type_completions(&self, schema: &Schema) -> Vec<CompletionItem> {
        let mut items = Vec::new();
        for (name, def) in &schema.types {
            let kind = match def {
                schema::ExtendedType::Object(_) | schema::ExtendedType::Interface(_) => {
                    Some(CompletionItemKind::CLASS)
                }
                schema::ExtendedType::Enum(_) => Some(CompletionItemKind::ENUM),
                schema::ExtendedType::Union(_) => Some(CompletionItemKind::INTERFACE),
                schema::ExtendedType::Scalar(_) => Some(CompletionItemKind::STRUCT),
                schema::ExtendedType::InputObject(_) => Some(CompletionItemKind::STRUCT),
            };
            items.push(CompletionItem {
                label: name.to_string(),
                kind,
                ..Default::default()
            });
        }
        items
    }

    fn get_node_text(&self, node: Node, offset: usize) -> String {
        let start = node.start_byte() + offset;
        let end = node.end_byte() + offset;
        self.rope.byte_slice(start..end).to_string()
    }

    pub fn get_semantic_diagnostics(&self, schema: &Schema) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        let blocks = self.get_graphql_trees();

        for (tree, offset) in blocks {
            let offset = *offset;
            // 1. Syntax errors
            self.collect_gql_errors(tree.root_node(), offset, &mut diagnostics);

            // 2. Schema validation
            self.validate_tree(tree.root_node(), offset, schema, &mut diagnostics);
        }
        diagnostics
    }

    fn validate_tree(
        &self,
        node: Node,
        offset: usize,
        schema: &Schema,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let query = GQL_VALIDATION_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_VALIDATION_QUERY).unwrap()
        });

        let mut cursor = QueryCursor::new();
        // Since we are validating a specific tree, we can run query on its root node
        // BUT matches expects text provider.
        let mut matches = cursor.matches(query, node, |n: Node| {
            let start = n.start_byte();
            let end = n.end_byte();
            self.rope
                .byte_slice((start + offset)..(end + offset))
                .chunks()
        });

        while let Some(m) = matches.next() {
            for cap in m.captures {
                let capture_name = query.capture_names()[cap.index as usize];
                if capture_name == "operation" {
                    self.validate_operation(cap.node, offset, schema, diagnostics);
                } else if capture_name == "fragment" {
                    self.validate_fragment(cap.node, offset, schema, diagnostics);
                }
            }
        }
    }

    fn validate_operation(
        &self,
        node: Node,
        offset: usize,
        schema: &Schema,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let mut operation_type_string = String::from("query");
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if child.kind() == "operation_type" {
                operation_type_string = self.get_node_text(child, offset);
                break;
            }
        }

        let op_type = match operation_type_string.as_str() {
            "query" => Some(apollo_compiler::ast::OperationType::Query),
            "mutation" => Some(apollo_compiler::ast::OperationType::Mutation),
            "subscription" => Some(apollo_compiler::ast::OperationType::Subscription),
            _ => None,
        };

        if let Some(op) = op_type {
            let root_def = schema.root_operation(op);
            if let Some(root_def_name) = root_def {
                if let Some(root_type) = schema.types.get(root_def_name.as_str()) {
                    for child in node.children(&mut cursor) {
                        if child.kind() == "selection_set" {
                            self.validate_selection_set(
                                child,
                                offset,
                                root_type,
                                schema,
                                diagnostics,
                            );
                        }
                    }
                }
            }
        }
    }

    fn validate_fragment(
        &self,
        node: Node,
        offset: usize,
        schema: &Schema,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        // fragment Name on Type { ... }
        // type_condition -> named_type -> name
        let mut cursor = node.walk();
        let mut type_condition_node = None;
        let mut selection_set_node = None;

        for child in node.children(&mut cursor) {
            if child.kind() == "type_condition" {
                type_condition_node = Some(child);
            } else if child.kind() == "selection_set" {
                selection_set_node = Some(child);
            }
        }

        if let Some(type_cond) = type_condition_node {
            // type_condition has child named_type which has child name
            let mut tc_cursor = type_cond.walk();
            for tc_child in type_cond.children(&mut tc_cursor) {
                if tc_child.kind() == "named_type" {
                    let mut nt_cursor = tc_child.walk();
                    for nt_child in tc_child.children(&mut nt_cursor) {
                        if nt_child.kind() == "name" {
                            let type_name = self.get_node_text(nt_child, offset);
                            if let Some(type_def) = schema.types.get(type_name.as_str()) {
                                if let Some(sel_set) = selection_set_node {
                                    self.validate_selection_set(
                                        sel_set,
                                        offset,
                                        type_def,
                                        schema,
                                        diagnostics,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn validate_selection_set(
        &self,
        selection_set: Node,
        offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let mut cursor = selection_set.walk();
        for child in selection_set.children(&mut cursor) {
            let kind = child.kind();

            if kind == "selection" {
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    let k = inner.kind();
                    if k == "field" {
                        self.validate_field(inner, offset, parent_type, schema, diagnostics);
                    } else if k == "inline_fragment" {
                        // TODO
                    } else if k == "fragment_spread" {
                        // TODO
                    }
                }
            } else if kind == "field" {
                self.validate_field(child, offset, parent_type, schema, diagnostics);
            }
        }
    }

    fn validate_field(
        &self,
        field_node: Node,
        offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let mut name_node = None;
        let mut selection_set_node = None;

        let mut cursor = field_node.walk();
        for child in field_node.children(&mut cursor) {
            if child.kind() == "name" {
                name_node = Some(child);
            } else if child.kind() == "selection_set" {
                selection_set_node = Some(child);
            }
        }

        if let Some(name_node) = name_node {
            let field_name = self.get_node_text(name_node, offset);

            if field_name == "__typename" {
                return;
            }

            let field_def = match parent_type {
                schema::ExtendedType::Object(obj) => obj.fields.get(field_name.as_str()),
                schema::ExtendedType::Interface(iface) => iface.fields.get(field_name.as_str()),
                _ => None,
            };

            if let Some(field_def) = field_def {
                // Check deprecation by directive

                if let Some(directive) = field_def.directives.get("deprecated") {
                    let reason = directive
                        .argument_by_name("reason", schema)
                        .ok()
                        .and_then(|arg| arg.as_str())
                        .unwrap_or("No reason provided");

                    diagnostics.push(Diagnostic {
                        range: self.translate_to_file_range(name_node, offset),

                        severity: Some(DiagnosticSeverity::WARNING),

                        message: format!("Field '{}' is deprecated: {}", field_name, reason),

                        ..Default::default()
                    });
                }

                if let Some(sel_set) = selection_set_node {
                    let field_type_name = field_def.ty.inner_named_type();
                    if let Some(field_type_def) = schema.types.get(field_type_name.as_str()) {
                        self.validate_selection_set(
                            sel_set,
                            offset,
                            field_type_def,
                            schema,
                            diagnostics,
                        );
                    }
                }
            } else {
                let type_name = match parent_type {
                    schema::ExtendedType::Object(o) => o.name.as_str(),
                    schema::ExtendedType::Interface(i) => i.name.as_str(),
                    schema::ExtendedType::Union(u) => u.name.as_str(),
                    schema::ExtendedType::Enum(e) => e.name.as_str(),
                    schema::ExtendedType::InputObject(i) => i.name.as_str(),
                    schema::ExtendedType::Scalar(s) => s.name.as_str(),
                };

                diagnostics.push(Diagnostic {
                    range: self.translate_to_file_range(name_node, offset),
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: format!("Field '{}' not found on type '{}'", field_name, type_name),
                    ..Default::default()
                });
            }
        }
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

    pub fn apply_change(
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

        self.graphql_trees = self.reparse_graphql_trees();
    }

    pub fn get_symbol_at_position(&self, position: Position) -> Option<String> {
        let char_idx = self.rope.line_to_char(position.line as usize) + position.character as usize;
        let byte_offset = self.rope.char_to_byte(char_idx);

        for (tree, offset) in self.get_graphql_trees() {
            let offset = *offset;
            let root = tree.root_node();
            let tree_len = root.end_byte();

            if byte_offset >= offset && byte_offset < offset + tree_len {
                let local_byte = byte_offset - offset;
                let trigger_node = root.descendant_for_byte_range(local_byte, local_byte)?;

                if trigger_node.kind() == "name" {
                    return Some(
                        self.rope
                            .slice(
                                self.rope.byte_to_char(trigger_node.start_byte() + offset)
                                    ..self.rope.byte_to_char(trigger_node.end_byte() + offset),
                            )
                            .to_string(),
                    );
                }
            }
        }
        None
    }

    pub fn find_definition_in_tree(&self, target_name: &str) -> Option<Location> {
        let query = GQL_DEFINITION_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_DEFINITION_QUERY).unwrap()
        });

        let mut cursor = tree_sitter::QueryCursor::new();

        // Search in ALL trees
        for (tree, offset) in self.get_graphql_trees() {
            let offset = *offset;
            let mut matches = cursor.matches(query, tree.root_node(), |node: Node| {
                let start = node.start_byte();
                let end = node.end_byte();
                // Careful: rope needs absolute bytes, but node has local bytes (0-based)
                // We must shift by offset
                self.rope
                    .byte_slice((start + offset)..(end + offset))
                    .chunks()
            });

            while let Some(m) = matches.next() {
                // We need to check if the captured name matches our target_name
                let name_node = m.captures[0].node;
                let name = self
                    .rope
                    .slice(
                        self.rope.byte_to_char(name_node.start_byte() + offset)
                            ..self.rope.byte_to_char(name_node.end_byte() + offset),
                    )
                    .to_string();

                if name == target_name {
                    let node = name_node;

                    // Translate local node range to file range
                    let range = self.translate_to_file_range(node, offset);
                    return Some(Location {
                        uri: self.uri.clone(),
                        range,
                    });
                }
            }
        }

        None
    }

    pub fn get_symbols(&self) -> Vec<DocumentSymbol> {
        let query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
        });

        let mut cursor = QueryCursor::new();
        let mut symbols = Vec::new();

        for (tree, offset) in self.get_graphql_trees() {
            let offset = *offset;
            // Execute query on the root node
            let mut matches = cursor.matches(query, tree.root_node(), |node: Node| {
                let start = node.start_byte();
                let end = node.end_byte();
                self.rope
                    .byte_slice((start + offset)..(end + offset))
                    .chunks()
            });

            while let Some(m) = matches.next() {
                // In our query, @symbol.name is the first capture (index 0)
                let name_node = m.captures[0].node;
                let container_node = m.captures[1].node;

                let name = self
                    .rope
                    .slice(
                        self.rope.byte_to_char(name_node.start_byte() + offset)
                            ..self.rope.byte_to_char(name_node.end_byte() + offset),
                    )
                    .to_string();

                #[allow(deprecated)]
                symbols.push(DocumentSymbol {
                    name,
                    detail: Some(format!("GraphQL {}", container_node.kind())),
                    kind: SymbolKind::STRUCT, // Map GQL types to LSP kinds
                    tags: None,
                    deprecated: None,
                    range: self.translate_to_file_range(container_node, offset),
                    selection_range: self.translate_to_file_range(name_node, offset),
                    children: None,
                });
            }
        }
        symbols
    }

    pub fn get_hover_info(&self, position: Position, schema: &Schema) -> Option<Hover> {
        let char_idx = self.rope.line_to_char(position.line as usize) + position.character as usize;
        let byte_offset = self.rope.char_to_byte(char_idx);

        for (tree, offset) in self.get_graphql_trees() {
            let offset = *offset;
            let root = tree.root_node();
            let tree_len = root.end_byte();

            if byte_offset >= offset && byte_offset < offset + tree_len {
                let local_byte = byte_offset - offset;
                let node = root.descendant_for_byte_range(local_byte, local_byte)?;

                // Only trigger hover on "name" nodes
                if node.kind() == "name" {
                    let symbol_name = self
                        .rope
                        .slice(
                            self.rope.byte_to_char(node.start_byte() + offset)
                                ..self.rope.byte_to_char(node.end_byte() + offset),
                        )
                        .to_string();

                    // 1. Try to get info from Schema
                    if let Some(schema_info) = self.get_type_info_from_schema(&symbol_name, schema)
                    {
                        return Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: schema_info,
                            }),
                            range: Some(self.translate_to_file_range(node, offset)),
                        });
                    }

                    // 2. Fallback: Find the definition and its description in local file
                    if let Some(description) = self.find_description(&symbol_name) {
                        return Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: format!("### {}\n---\n{}", symbol_name, description),
                            }),
                            range: Some(self.translate_to_file_range(node, offset)),
                        });
                    }
                }
            }
        }
        None
    }

    fn get_type_info_from_schema(&self, name: &str, schema: &Schema) -> Option<String> {
        // apollo-compiler uses Name which is a wrapper around string
        let ty = schema.types.get(name)?;

        let mut output = String::new();

        // Header
        match ty {
            schema::ExtendedType::Scalar(_) => output.push_str(&format!("### scalar {}\n", name)),
            schema::ExtendedType::Object(_) => output.push_str(&format!("### type {}\n", name)),
            schema::ExtendedType::Interface(_) => {
                output.push_str(&format!("### interface {}\n", name))
            }
            schema::ExtendedType::Union(_) => output.push_str(&format!("### union {}\n", name)),
            schema::ExtendedType::Enum(_) => output.push_str(&format!("### enum {}\n", name)),
            schema::ExtendedType::InputObject(_) => {
                output.push_str(&format!("### input {}\n", name))
            }
        }

        output.push_str("---\n");

        if let Some(desc) = ty.description() {
            output.push_str(desc);
            output.push_str("\n\n");
        }

        // Add Fields or Enum Values
        match ty {
            schema::ExtendedType::Object(obj) => {
                output.push_str("#### Fields\n");
                for (field_name, field_def) in &obj.fields {
                    output.push_str(&format!("- **{}**: `{}`\n", field_name, field_def.ty));
                }
            }
            schema::ExtendedType::Interface(iface) => {
                output.push_str("#### Fields\n");
                for (field_name, field_def) in &iface.fields {
                    output.push_str(&format!("- **{}**: `{}`\n", field_name, field_def.ty));
                }
            }
            schema::ExtendedType::InputObject(input) => {
                output.push_str("#### Fields\n");
                for (field_name, field_def) in &input.fields {
                    output.push_str(&format!("- **{}**: `{}`\n", field_name, field_def.ty));
                }
            }
            schema::ExtendedType::Enum(enm) => {
                output.push_str("#### Values\n");
                for (val_name, _) in &enm.values {
                    output.push_str(&format!("- `{}`\n", val_name));
                }
            }
            _ => {}
        }

        Some(output)
    }

    fn find_description(&self, target_name: &str) -> Option<String> {
        let query = GQL_DESCRIPTION_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_DESCRIPTION_QUERY).unwrap()
        });

        let mut cursor = tree_sitter::QueryCursor::new();

        for (tree, offset) in self.get_graphql_trees() {
            let offset = *offset;
            let mut matches = cursor.matches(query, tree.root_node(), |node: Node| {
                let start = node.start_byte();
                let end = node.end_byte();
                self.rope
                    .byte_slice((start + offset)..(end + offset))
                    .chunks()
            });

            while let Some(m) = matches.next() {
                // Check if the captured name matches our target_name
                let mut desc_node = None;
                let mut name_node = None;

                for capture in m.captures {
                    let capture_name = query.capture_names()[capture.index as usize];
                    if capture_name == "desc" {
                        desc_node = Some(capture.node);
                    } else if capture_name == "name" {
                        name_node = Some(capture.node);
                    }
                }

                if let Some(n_node) = name_node {
                    let name = self
                        .rope
                        .slice(
                            self.rope.byte_to_char(n_node.start_byte() + offset)
                                ..self.rope.byte_to_char(n_node.end_byte() + offset),
                        )
                        .to_string();

                    if name == target_name {
                        if let Some(d_node) = desc_node {
                            return Some(
                                self.rope
                                    .slice(
                                        self.rope.byte_to_char(d_node.start_byte() + offset)
                                            ..self.rope.byte_to_char(d_node.end_byte() + offset),
                                    )
                                    .to_string()
                                    .trim_matches('"')
                                    .to_string(),
                            );
                        } else {
                            // Found the type but it has no description
                            return None;
                        }
                    }
                }
            }
        }
        None
    }

    pub fn get_semantic_tokens(&self) -> Vec<SemanticToken> {
        let query = GQL_SEMANTIC_TOKEN_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, SEMANTIC_TOKEN_QUERY).unwrap()
        });

        let mut cursor = QueryCursor::new();

        let mut tokens = Vec::new();

        for (tree, offset) in self.get_graphql_trees() {
            let offset = *offset;
            let mut matches = cursor.matches(query, tree.root_node(), |node: Node| {
                let start = node.start_byte();
                let end = node.end_byte();
                self.rope
                    .byte_slice((start + offset)..(end + offset))
                    .chunks()
            });

            while let Some(m) = matches.next() {
                for cap in m.captures {
                    let token_type_name = &query.capture_names()[cap.index as usize];
                    let token_type = token_type_to_legend_index(token_type_name);

                    let range = self.translate_to_file_range(cap.node, offset);

                    tokens.push(SemanticToken {
                        delta_line: range.start.line,
                        delta_start: range.start.character,
                        length: range.end.character - range.start.character,
                        token_type,
                        token_modifiers_bitset: 0,
                    });
                }
            }
        }

        tokens
    }
}

pub fn token_type_to_legend_index(token_type: &str) -> u32 {
    match token_type {
        "variable" => 0,
        "type" => 1,
        "keyword" => 2,
        "enum" => 3,
        "string" => 4,
        _ => 0,
    }
}

pub struct Backend {
    pub client: Client,
    pub documents: DashMap<Url, DocumentState>,
    pub schema: Arc<RwLock<Schema>>,
}

impl Backend {
    pub fn new(client: Client, schema_path: &str) -> Self {
        let schema_text = std::fs::read_to_string(schema_path).unwrap_or_else(|_| "".to_string());
        let schema =
            Schema::parse(&schema_text, schema_path).expect("Failed to parse initial schema");

        Self {
            client,
            documents: DashMap::new(),
            schema: Arc::new(RwLock::new(schema)),
        }
    }

    async fn on_schema_file_changed(&self, new_text: &str) {
        let new_schema = Schema::parse(new_text, "schema.graphql").unwrap();
        {
            let mut lock = self.schema.write().unwrap();
            *lock = new_schema;
        }

        self.client
            .log_message(MessageType::INFO, "Schema updated!")
            .await;
    }

    async fn reload_schema(&self, path: &str) {
        if let Ok(text) = std::fs::read_to_string(path) {
            match Schema::parse(&text, path) {
                Ok(new_schema) => {
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

pub const SEMANTIC_TOKEN_LEGEND: &[SemanticTokenType] = &[
    SemanticTokenType::VARIABLE,
    SemanticTokenType::TYPE,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::ENUM,
    SemanticTokenType::STRING,
];

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
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
            let schema = self.schema.read().unwrap();
            return Ok(doc.get_hover_info(position, &schema));
        }

        Ok(None)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        if let Some(doc) = self.documents.get(uri) {
            let schema = self.schema.read().unwrap();
            let items = doc.get_completion_items(position, &schema);
            return Ok(Some(CompletionResponse::Array(items)));
        }

        Ok(None)
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;

        if let Some(mut doc) = self.documents.get_mut(&uri) {
            let mut parser = tree_sitter::Parser::new();
            parser
                .set_language(&doc.language.get_parser_language())
                .unwrap();

            for change in params.content_changes {
                doc.apply_change(&change, &mut parser);
            }

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

        // 1. Find the symbol name at the cursor
        let symbol_name = if let Some(doc) = self.documents.get(&uri) {
            doc.get_symbol_at_position(position)
        } else {
            None
        };

        // 2. Search for definition in all documents
        if let Some(name) = symbol_name {
            for entry in self.documents.iter() {
                let doc = entry.value();
                if let Some(location) = doc.find_definition_in_tree(&name) {
                    return Ok(Some(GotoDefinitionResponse::Scalar(location)));
                }
            }
        }

        Ok(None)
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let language = DocumentLanguage::from_uri(&params.text_document.uri);
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&language.get_parser_language())
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

        let config = self
            .client
            .configuration(vec![ConfigurationItem {
                scope_uri: None,
                section: Some("gqlLsp.schemaPath".to_string()),
            }])
            .await;

        if let Ok(values) = config
            && let Some(path_value) = values.first().and_then(|v| v.as_str())
        {
            self.client
                .log_message(
                    MessageType::INFO,
                    format!("New schema path: {}", path_value),
                )
                .await;

            self.reload_schema(path_value).await;
        }
    }
}
