use crate::queries::*;
use crate::utils::{find_package_root, mask_interpolations};
use apollo_compiler::Schema;
use apollo_compiler::schema::ExtendedType;
use lsp_types::*;
use ropey::Rope;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use tree_sitter::{InputEdit, Node, Parser, Point, StreamingIterator, Tree};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentLanguage {
    GraphQL,
    TypeScript,
    TSX,
}

impl DocumentLanguage {
    pub fn from_uri(uri: &Url) -> Self {
        let path = uri.path();
        if path.ends_with(".tsx") || path.ends_with(".jsx") {
            DocumentLanguage::TSX
        } else if path.ends_with(".ts")
            || path.ends_with(".cts")
            || path.ends_with(".mts")
            || path.ends_with(".js")
            || path.ends_with(".mjs")
            || path.ends_with(".cjs")
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
            DocumentLanguage::TSX => tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }

    pub fn is_host_language(&self) -> bool {
        matches!(self, DocumentLanguage::TypeScript | DocumentLanguage::TSX)
    }
}

#[derive(Clone)]
pub struct GraphQLBlock {
    pub tree: Arc<Tree>,
    pub offset: usize,
}

impl fmt::Debug for GraphQLBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GraphQLBlock")
            .field("offset", &self.offset)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct FragmentDef {
    pub name: Arc<str>,
    pub type_condition: Arc<str>,
    pub is_public: bool,
    pub is_type_only: bool,
    pub description: Option<Arc<str>>,
    pub source_hash: u64,
    pub used_variables: Vec<Arc<str>>,
    pub used_fragments: Vec<Arc<str>>,
}

#[derive(Debug, Clone)]
pub struct OperationDef {
    pub name: Option<Arc<str>>,
    pub operation_type: Arc<str>,
    pub source_text: Arc<str>,
}

/// Components of a GraphQL field node.
#[derive(Default)]
pub struct FieldComponents<'a> {
    pub alias: Option<Node<'a>>,
    pub name: Option<Node<'a>>,
    pub selection_set: Option<Node<'a>>,
    pub arguments: Option<Node<'a>>,
    pub directives: Option<Node<'a>>,
}

/// Components of a GraphQL variable definition node.
#[derive(Default)]
pub struct VariableDefinitionComponents<'a> {
    pub variable: Option<Node<'a>>,
    pub type_node: Option<Node<'a>>,
    pub default_value: Option<Node<'a>>,
    pub directives: Option<Node<'a>>,
}

/// Components of a GraphQL argument or object field node.
#[derive(Default)]
pub struct NamedValueComponents<'a> {
    pub name: Option<Node<'a>>,
    pub value: Option<Node<'a>>,
}

#[derive(Debug, Clone)]
pub struct DocumentState {
    pub uri: Url,
    pub rope: Rope,
    pub tree: Arc<Tree>,
    pub language: DocumentLanguage,
    pub graphql_trees: Vec<GraphQLBlock>,
    pub fragments: Vec<FragmentDef>,
    pub fragment_spreads: Vec<Arc<str>>,
    pub operations: Vec<OperationDef>,
    pub package_root: Option<PathBuf>,
    pub masked_source: Arc<str>,
    pub version: i32,
    pub mtime: Option<std::time::SystemTime>,
    pub position_encoding: PositionEncodingKind,
}

impl DocumentState {
    pub fn new(
        uri: Url,
        text: &str,
        mut parser: tree_sitter::Parser,
        position_encoding: PositionEncodingKind,
    ) -> Self {
        let language = DocumentLanguage::from_uri(&uri);
        let rope = Rope::from_str(text);
        let tree = Arc::new(parser.parse(text, None).unwrap());
        let (package_root, mtime) = if let Ok(path) = uri.to_file_path() {
            (
                find_package_root(&path),
                std::fs::metadata(&path)
                    .ok()
                    .and_then(|m| m.modified().ok()),
            )
        } else {
            (None, None)
        };

        let masked_source = if language.is_host_language() {
            mask_interpolations(text).into()
        } else {
            text.into()
        };

        let mut doc = Self {
            uri,
            rope,
            tree,
            language,
            graphql_trees: Vec::new(),
            fragments: Vec::new(),
            fragment_spreads: Vec::new(),
            operations: Vec::new(),
            package_root,
            masked_source,
            version: 0,
            mtime,
            position_encoding,
        };
        doc.graphql_trees = doc.reparse_graphql_trees();
        let (fragments, operations, spreads) = doc.extract_symbols();
        doc.fragments = fragments;
        doc.operations = operations;
        doc.fragment_spreads = spreads;
        doc
    }

    pub fn reparse_graphql_trees(&self) -> Vec<GraphQLBlock> {
        if self.language == DocumentLanguage::GraphQL {
            return vec![GraphQLBlock {
                tree: self.tree.clone(),
                offset: 0,
            }];
        }

        if !self.has_graphql_candidates() {
            return vec![];
        }

        let query = match self.language {
            DocumentLanguage::TSX => TSX_QUERY_CACHE.get_or_init(|| {
                let lang = tree_sitter_typescript::LANGUAGE_TSX.into();
                tree_sitter::Query::new(&lang, TS_GQL_QUERY).unwrap()
            }),
            _ => TS_QUERY_CACHE.get_or_init(|| {
                let lang = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
                tree_sitter::Query::new(&lang, TS_GQL_QUERY).unwrap()
            }),
        };

        let gql_content_idx = query.capture_index_for_name("gql_content").unwrap();
        let gql_template_idx = query.capture_index_for_name("gql_template").unwrap();

        let mut cursor = tree_sitter::QueryCursor::new();

        let mut ts_matches = cursor.matches(query, self.tree.root_node(), |node: Node| {
            let start = node.start_byte();
            let end = node.end_byte();

            self.rope.byte_slice(start..end).chunks()
        });

        let mut gql_blocks = vec![];
        let mut seen_nodes = ahash::AHashSet::default();

        let mut gql_parser = Parser::new();
        gql_parser
            .set_language(&tree_sitter_graphql::LANGUAGE.into())
            .unwrap();

        while let Some(m) = ts_matches.next() {
            let mut gql_node = None;

            for i in 0..m.captures.len() {
                let cap = &m.captures[i];
                if cap.index == gql_content_idx {
                    gql_node = Some(cap.node);
                    break;
                } else if cap.index == gql_template_idx {
                    let node = cap.node;
                    let mut curr = node.prev_named_sibling();
                    while let Some(prev) = curr {
                        if prev.kind() == "comment" {
                            let text = self.get_node_text(prev, 0);
                            if text
                                .as_bytes()
                                .windows(7)
                                .any(|w| w.eq_ignore_ascii_case(b"graphql"))
                            {
                                gql_node = Some(node);
                                break;
                            }
                        }
                        if prev.kind() != "comment" {
                            break;
                        }
                        curr = prev.prev_named_sibling();
                    }
                    if gql_node.is_some() {
                        break;
                    }
                }
            }

            if let Some(node) = gql_node {
                if !seen_nodes.insert(node.id()) {
                    continue;
                }

                // For template literals, the content is inside backticks. We compute
                // start/end bytes for the inner content and verify they are within bounds.
                let mut start_byte = node.start_byte() + 1;
                let mut end_byte = node.end_byte() - 1;

                if start_byte >= end_byte {
                    let mut found = false;
                    for i in 0..node.named_child_count() {
                        if let Some(child) = node.named_child(i as u32) {
                            let kind = child.kind();
                            if (kind.contains("template") || kind.contains("string"))
                                && child.end_byte() > child.start_byte() + 2
                            {
                                start_byte = child.start_byte() + 1;
                                end_byte = child.end_byte() - 1;
                                found = true;
                                break;
                            }
                        }
                    }

                    if !found && let Some(child) = node.named_child(0u32) {
                        start_byte = child.start_byte() + 1;
                        end_byte = child.end_byte() - 1;
                    }
                }

                let doc_len = self.rope.len_bytes();
                if start_byte > doc_len || end_byte > doc_len || start_byte >= end_byte {
                    continue;
                }

                let raw_gql = self.rope.byte_slice(start_byte..end_byte).to_string();
                let masked_gql = mask_interpolations(&raw_gql);

                if let Some(gql_tree) = gql_parser.parse(&masked_gql, None) {
                    gql_blocks.push(GraphQLBlock {
                        tree: Arc::new(gql_tree),
                        offset: start_byte,
                    });
                }
            }
        }

        gql_blocks
    }

    pub fn get_graphql_trees(&self) -> &[GraphQLBlock] {
        &self.graphql_trees
    }

    pub fn translate_to_file_range(
        &self,
        gql_node: tree_sitter::Node,
        offset_byte: usize,
    ) -> Range {
        let absolute_start_byte = gql_node.start_byte() + offset_byte;
        let absolute_end_byte = gql_node.end_byte() + offset_byte;

        Range {
            start: self.byte_to_position(absolute_start_byte),
            end: self.byte_to_position(absolute_end_byte),
        }
    }

    pub fn byte_to_position(&self, byte_offset: usize) -> Position {
        let line = self.rope.byte_to_line(byte_offset);
        let line_start_byte = self.rope.line_to_byte(line);

        let character = if self.position_encoding == PositionEncodingKind::UTF8 {
            byte_offset - line_start_byte
        } else if self.position_encoding == PositionEncodingKind::UTF16 {
            let char_at_offset = self.rope.byte_to_char(byte_offset);
            let char_at_line_start = self.rope.byte_to_char(line_start_byte);

            let utf16_cu_at_offset = self.rope.char_to_utf16_cu(char_at_offset);
            let utf16_cu_at_line_start = self.rope.char_to_utf16_cu(char_at_line_start);

            utf16_cu_at_offset - utf16_cu_at_line_start
        } else {
            let char_at_offset = self.rope.byte_to_char(byte_offset);
            let char_at_line_start = self.rope.byte_to_char(line_start_byte);
            char_at_offset - char_at_line_start
        };

        Position::new(line as u32, character as u32)
    }

    pub fn position_to_byte(&self, position: Position) -> usize {
        let line_idx = position.line as usize;
        if line_idx >= self.rope.len_lines() {
            return self.rope.len_bytes();
        }

        if self.position_encoding == PositionEncodingKind::UTF8 {
            let line_start_byte = self.rope.line_to_byte(line_idx);
            let target_byte = line_start_byte + position.character as usize;
            let next_line_start_byte = if line_idx + 1 < self.rope.len_lines() {
                self.rope.line_to_byte(line_idx + 1)
            } else {
                self.rope.len_bytes()
            };

            if target_byte >= next_line_start_byte {
                next_line_start_byte.saturating_sub(1)
            } else {
                target_byte
            }
        } else if self.position_encoding == PositionEncodingKind::UTF16 {
            let line_start_char = self.rope.line_to_char(line_idx);
            let line_start_utf16_cu = self.rope.char_to_utf16_cu(line_start_char);

            let target_utf16_cu = line_start_utf16_cu + position.character as usize;
            let len_utf16_cu = self.rope.len_utf16_cu();

            let target_char = if target_utf16_cu >= len_utf16_cu {
                self.rope.len_chars()
            } else {
                self.rope.utf16_cu_to_char(target_utf16_cu)
            };
            self.rope.char_to_byte(target_char)
        } else {
            let line_start_char = self.rope.line_to_char(line_idx);
            let target_char = line_start_char + position.character as usize;

            let next_line_start_char = if line_idx + 1 < self.rope.len_lines() {
                self.rope.line_to_char(line_idx + 1)
            } else {
                self.rope.len_chars()
            };

            let clamped_target_char = if target_char >= next_line_start_char {
                next_line_start_char.saturating_sub(1)
            } else {
                target_char
            };

            self.rope.char_to_byte(clamped_target_char)
        }
    }

    pub fn has_graphql_candidates(&self) -> bool {
        if self.language == DocumentLanguage::GraphQL {
            return true;
        }
        self.rope.chunks().any(|chunk| {
            let bytes = chunk.as_bytes();
            bytes.windows(3).any(|w| w.eq_ignore_ascii_case(b"gql"))
                || bytes.windows(7).any(|w| w.eq_ignore_ascii_case(b"graphql"))
        })
    }

    pub fn get_node_text(&self, node: Node, offset: usize) -> String {
        let start = node.start_byte() + offset;
        let end = node.end_byte() + offset;
        self.rope.byte_slice(start..end).to_string()
    }

    pub fn get_fragment_type_condition(&self, node: Node, offset: usize) -> Option<String> {
        self.find_child_by_kind(node, "type_condition")
            .and_then(|tc| self.find_child_by_kind(tc, "named_type"))
            .and_then(|nt| self.find_child_by_kind(nt, "name"))
            .map(|name| self.get_node_text(name, offset))
    }

    // ========================================================================
    // Tree-sitter Helper Functions
    // ========================================================================

    /// Finds the first child node of the specified kind.
    pub fn find_child_by_kind<'a>(&self, node: Node<'a>, kind: &str) -> Option<Node<'a>> {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .find(|&child| child.kind() == kind)
    }

    pub fn find_ancestor_by_kind<'a>(&self, node: Node<'a>, target_kind: &str) -> Option<Node<'a>> {
        self.find_ancestor_by_kinds(node, &[target_kind])
    }

    pub fn find_ancestor_by_kinds<'a>(
        &self,
        node: Node<'a>,
        target_kinds: &[&str],
    ) -> Option<Node<'a>> {
        let mut current = node;
        while let Some(parent) = current.parent() {
            if target_kinds.contains(&parent.kind()) {
                return Some(parent);
            }
            current = parent;
        }
        None
    }

    pub fn skip_through_kinds<'a>(
        &self,
        mut node: Node<'a>,
        skip_kinds: &[&str],
    ) -> Option<Node<'a>> {
        while skip_kinds.contains(&node.kind()) {
            if let Some(parent) = node.parent() {
                node = parent;
            } else {
                return None;
            }
        }
        Some(node)
    }

    pub fn is_cursor_in_node_range(&self, node: Node, offset: usize, cursor_offset: usize) -> bool {
        let start = node.start_byte() + offset;
        let end = node.end_byte() + offset;
        cursor_offset >= start && cursor_offset <= end
    }

    pub fn hash_node_text(&self, node: Node, offset: usize) -> u64 {
        let start = node.start_byte() + offset;
        let end = node.end_byte() + offset;
        let mut hasher = ahash::AHasher::default();
        use std::hash::Hasher;
        for chunk in self.rope.byte_slice(start..end).chunks() {
            hasher.write(chunk.as_bytes());
        }
        hasher.finish()
    }

    pub fn get_operation_type(&self, operation_node: Node, offset: usize) -> String {
        let mut cursor = operation_node.walk();
        for child in operation_node.children(&mut cursor) {
            if child.kind() == "operation_type" {
                return self
                    .rope
                    .slice(
                        self.rope.byte_to_char(child.start_byte() + offset)
                            ..self.rope.byte_to_char(child.end_byte() + offset),
                    )
                    .to_string();
            }
        }
        "query".to_string()
    }

    pub fn extract_field_components<'a>(&self, field_node: Node<'a>) -> FieldComponents<'a> {
        let mut components = FieldComponents::default();
        let mut cursor = field_node.walk();

        for child in field_node.children(&mut cursor) {
            match child.kind() {
                "alias" => components.alias = Some(child),
                "name" => components.name = Some(child),
                "selection_set" => components.selection_set = Some(child),
                "arguments" => components.arguments = Some(child),
                "directives" => components.directives = Some(child),
                _ => {}
            }
        }

        components
    }

    pub fn extract_variable_definition_components<'a>(
        &self,
        vd_node: Node<'a>,
    ) -> VariableDefinitionComponents<'a> {
        let mut components = VariableDefinitionComponents::default();
        let mut cursor = vd_node.walk();

        for child in vd_node.children(&mut cursor) {
            match child.kind() {
                "variable" => components.variable = Some(child),
                "type" => components.type_node = Some(child),
                "default_value" => components.default_value = Some(child),
                "directives" => components.directives = Some(child),
                _ => {}
            }
        }

        components
    }

    pub fn extract_named_value_components<'a>(&self, node: Node<'a>) -> NamedValueComponents<'a> {
        let mut components = NamedValueComponents::default();
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            match child.kind() {
                "name" => components.name = Some(child),
                "value" | "variable" => components.value = Some(child),
                _ => {
                    if child.kind().ends_with("_value") {
                        components.value = Some(child);
                    }
                }
            }
        }

        components
    }

    pub fn fragments(&self) -> &[FragmentDef] {
        &self.fragments
    }

    pub fn operations(&self) -> &[OperationDef] {
        &self.operations
    }

    pub fn extract_symbols(&self) -> (Vec<FragmentDef>, Vec<OperationDef>, Vec<Arc<str>>) {
        let symbol_query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
        });

        let ref_query = GQL_REFERENCES_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_REFERENCES_QUERY).unwrap()
        });

        let mut cursor = tree_sitter::QueryCursor::new();
        let mut operations = Vec::new();
        let mut all_fragment_spreads = Vec::new();

        // Temporary storage for fragments to be enriched with references later
        struct PartialFragment {
            def: FragmentDef,
            start: usize,
            end: usize,
        }
        let mut partial_fragments = Vec::new();

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let mut matches = cursor.matches(symbol_query, block.tree.root_node(), |node: Node| {
                let start = node.start_byte();
                let end = node.end_byte();
                self.rope
                    .byte_slice((start + offset)..(end + offset))
                    .chunks()
            });

            while let Some(m) = matches.next() {
                let mut name = None;
                let mut type_condition = None;
                let mut is_fragment = false;
                let mut is_operation = false;
                let mut is_public = false;
                let mut is_type_only = false;
                let mut description = None;
                let mut container_node = None;
                let mut op_type: Arc<str> = "query".into();

                for cap in m.captures {
                    let cap_name = symbol_query.capture_names()[cap.index as usize];
                    match cap_name {
                        "symbol.name" => {
                            name = Some(self.get_node_text(cap.node, offset));
                        }
                        "symbol.type_condition" => {
                            type_condition = Some(self.get_node_text(cap.node, offset));
                        }
                        "symbol.container" => {
                            container_node = Some(cap.node);
                            match cap.node.kind() {
                                "fragment_definition" => is_fragment = true,
                                "operation_definition" => {
                                    is_operation = true;
                                    if let Some(ot_node) =
                                        self.find_child_by_kind(cap.node, "operation_type")
                                    {
                                        op_type = self.get_node_text(ot_node, offset).into();
                                    }
                                }
                                _ => {}
                            }
                        }
                        "symbol.directives" => {
                            let directives_text = self.get_node_text(cap.node, offset);
                            if directives_text.contains("@public") {
                                is_public = true;
                            }
                            if directives_text.contains("@type_only") {
                                is_type_only = true;
                            }
                        }
                        _ => {}
                    }
                }

                if let Some(container) = container_node {
                    if is_fragment && let Some(n) = name {
                        let source_hash = self.hash_node_text(container, offset);

                        let mut walker = container.walk();
                        for child in container.children(&mut walker) {
                            if child.kind() == "description" {
                                if let Some(sv) = child.child_by_field_name("content") {
                                    description = Some(
                                        self.get_node_text(sv, offset).trim_matches('"').into(),
                                    );
                                } else if let Some(sv) = child.child(0) {
                                    description = Some(
                                        self.get_node_text(sv, offset).trim_matches('"').into(),
                                    );
                                }
                            }
                        }

                        if description.is_none() {
                            let range = self.translate_to_file_range(container, offset);
                            if range.start.line > 0 {
                                let prev_line_num = range.start.line - 1;
                                let line_start = self.rope.line_to_char(prev_line_num as usize);
                                let line_end = self.rope.line_to_char(range.start.line as usize);
                                let line_text = self.rope.slice(line_start..line_end).to_string();
                                let trimmed = line_text.trim();
                                if trimmed.starts_with('#') {
                                    description =
                                        Some(trimmed.trim_start_matches('#').trim().into());
                                }
                            }
                        }

                        partial_fragments.push(PartialFragment {
                            def: FragmentDef {
                                name: n.into(),
                                type_condition: type_condition.unwrap_or_default().into(),
                                is_public,
                                is_type_only,
                                description,
                                source_hash,
                                used_variables: Vec::new(),
                                used_fragments: Vec::new(),
                            },
                            start: container.start_byte() + offset,
                            end: container.end_byte() + offset,
                        });
                    } else if is_operation {
                        let source_text = self.get_node_text(container, offset).into();
                        operations.push(OperationDef {
                            name: name.map(|n| n.into()),
                            operation_type: op_type,
                            source_text,
                        });
                    }
                }
            }
        }

        // Second pass: Extract all references once and attribute them to fragments
        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let mut ref_matches =
                cursor.matches(ref_query, block.tree.root_node(), |node: Node| {
                    let start = node.start_byte();
                    let end = node.end_byte();
                    self.rope
                        .byte_slice((start + offset)..(end + offset))
                        .chunks()
                });

            let reference_idx = ref_query.capture_index_for_name("reference").unwrap();
            let name_idx = ref_query.capture_index_for_name("name").unwrap();

            let mut current_pf_idx = 0;

            while let Some(rm) = ref_matches.next() {
                let mut is_reference = false;
                let mut name_node = None;

                for cap in rm.captures {
                    if cap.index == reference_idx {
                        is_reference = true;
                    } else if cap.index == name_idx {
                        name_node = Some(cap.node);
                    }
                }

                if is_reference && let Some(nn) = name_node {
                    let abs_start = nn.start_byte() + offset;

                    let is_variable = self
                        .rope
                        .get_char(self.rope.byte_to_char(abs_start))
                        .is_some_and(|c| c == '$');

                    let mut is_fragment_spread = false;
                    let mut node_text_cache: Option<Arc<str>> = None;

                    if !is_variable
                        && let Some(parent) = nn.parent()
                        && parent.kind() == "fragment_name"
                    {
                        let text: Arc<str> = self.get_node_text(nn, offset).into();
                        all_fragment_spreads.push(text.clone());
                        node_text_cache = Some(text);
                        is_fragment_spread = true;
                    }

                    // Attribute to fragment if it's inside one.
                    // Since both matches and partial_fragments are sorted by position,
                    // we can efficiently find the containing fragment.
                    while current_pf_idx < partial_fragments.len()
                        && abs_start >= partial_fragments[current_pf_idx].end
                    {
                        current_pf_idx += 1;
                    }

                    if current_pf_idx < partial_fragments.len() {
                        let pf = &mut partial_fragments[current_pf_idx];
                        if abs_start >= pf.start && abs_start < pf.end {
                            if is_variable {
                                let mut v_cursor = nn.walk();
                                for v_child in nn.children(&mut v_cursor) {
                                    if v_child.kind() == "name" {
                                        pf.def
                                            .used_variables
                                            .push(self.get_node_text(v_child, offset).into());
                                    }
                                }
                            } else if is_fragment_spread {
                                let text = node_text_cache
                                    .get_or_insert_with(|| self.get_node_text(nn, offset).into());
                                pf.def.used_fragments.push(text.clone());
                            }
                        }
                    }
                }
            }
        }

        let fragments = partial_fragments.into_iter().map(|pf| pf.def).collect();

        (fragments, operations, all_fragment_spreads)
    }

    pub fn get_fragment_spreads_in_node(&self, node: Node, offset: usize) -> Vec<Arc<str>> {
        let query = GQL_REFERENCES_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_REFERENCES_QUERY).unwrap()
        });

        let mut cursor = tree_sitter::QueryCursor::new();
        let mut spreads = Vec::new();

        let reference_idx = query.capture_index_for_name("reference").unwrap();

        let mut matches = cursor.matches(query, node, |node: Node| {
            let start = node.start_byte();
            let end = node.end_byte();
            self.rope
                .byte_slice((start + offset)..(end + offset))
                .chunks()
        });

        while let Some(m) = matches.next() {
            let mut is_reference = false;
            let mut name_node = None;

            for cap in m.captures {
                if cap.index == reference_idx {
                    is_reference = true;
                } else if query.capture_names()[cap.index as usize] == "name" {
                    name_node = Some(cap.node);
                }
            }

            if is_reference && let Some(name_node) = name_node {
                let name = self.get_node_text(name_node, offset);
                if !name.starts_with('$')
                    && let Some(parent) = name_node.parent()
                    && parent.kind() == "fragment_name"
                {
                    spreads.push(name.into());
                }
            }
        }
        spreads
    }

    pub fn find_fragment_info(&self, target_name: &str) -> Option<String> {
        let query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
        });

        let mut cursor = tree_sitter::QueryCursor::new();

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let mut matches = cursor.matches(query, block.tree.root_node(), |node: Node| {
                let start = node.start_byte();
                let end = node.end_byte();
                self.rope
                    .byte_slice((start + offset)..(end + offset))
                    .chunks()
            });

            while let Some(m) = matches.next() {
                let mut name = None;
                let mut is_fragment = false;
                let mut container_node = None;

                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    if cap_name == "symbol.name" {
                        name = Some(self.get_node_text(cap.node, offset));
                    } else if cap_name == "symbol.container"
                        && cap.node.kind() == "fragment_definition"
                    {
                        is_fragment = true;
                        container_node = Some(cap.node);
                    }
                }

                if is_fragment
                    && let Some(n) = name
                    && n == target_name
                    && let Some(cont) = container_node
                {
                    return Some(self.get_node_text(cont, offset));
                }
            }
        }
        None
    }

    pub fn apply_change(
        &mut self,
        change: &TextDocumentContentChangeEvent,
        parser: &mut Parser,
        version: i32,
    ) {
        // Update version
        self.version = version;

        if let Some(range) = change.range {
            let start_byte = self.position_to_byte(range.start);
            let old_end_byte = self.position_to_byte(range.end);

            let start_char = self.rope.byte_to_char(start_byte);
            let end_char = self.rope.byte_to_char(old_end_byte);

            self.rope.remove(start_char..end_char);
            self.rope.insert(start_char, &change.text);

            let new_end_byte = start_byte + change.text.len();
            let new_end_char = start_char + change.text.chars().count();
            let new_end_line = self.rope.char_to_line(new_end_char);

            let line_start_char = self.rope.line_to_char(new_end_line);
            let line_start_utf16 = self.rope.char_to_utf16_cu(line_start_char);
            let current_utf16 = self.rope.char_to_utf16_cu(new_end_char);
            let new_end_col_utf16 = current_utf16 - line_start_utf16;

            let edit = InputEdit {
                start_byte,
                old_end_byte,
                new_end_byte,
                start_position: Point::new(
                    range.start.line as usize,
                    range.start.character as usize,
                ),
                old_end_position: Point::new(range.end.line as usize, range.end.character as usize),
                new_end_position: Point::new(new_end_line, new_end_col_utf16),
            };

            if Arc::strong_count(&self.tree) > 1 {
                Arc::make_mut(&mut self.tree);
            }

            if let Some(tree) = Arc::get_mut(&mut self.tree) {
                tree.edit(&edit);
                self.tree = Arc::new(
                    parser
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
                        .unwrap(),
                );
            } else {
                let full_text = self.rope.to_string();
                self.tree = Arc::new(parser.parse(&full_text, None).unwrap());
            }
        } else {
            self.rope = Rope::from_str(&change.text);
            self.tree = Arc::new(parser.parse(&change.text, None).unwrap());
        }

        self.graphql_trees = self.reparse_graphql_trees();
        let (fragments, operations, spreads) = self.extract_symbols();
        self.fragments = fragments;
        self.operations = operations;
        self.fragment_spreads = spreads;

        // Only remask if language uses interpolations (TS/TSX)
        self.masked_source = if self.language.is_host_language() {
            mask_interpolations(&self.rope.to_string()).into()
        } else {
            self.rope.to_string().into()
        };
    }

    pub fn get_symbol_at_position(&self, position: Position) -> Option<String> {
        let byte_offset = self.position_to_byte(position);
        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();
            let tree_len = root.end_byte();
            if byte_offset >= offset && byte_offset < offset + tree_len {
                let local_byte = byte_offset - offset;
                let node = root.descendant_for_byte_range(local_byte, local_byte)?;
                if node.kind() == "name"
                    && let Some(parent) = node.parent()
                    && parent.kind() == "variable"
                {
                    return Some(self.get_node_text(parent, offset));
                }
                if node.kind() == "name"
                    && let Some(parent) = node.parent()
                    && (parent.kind() == "directive" || parent.kind() == "directive_definition")
                {
                    return Some(format!("@{}", self.get_node_text(node, offset)));
                }
                if node.kind() == "name" || node.kind() == "variable" {
                    return Some(self.get_node_text(node, offset));
                }
                if node.kind() == "@"
                    && let Some(parent) = node.parent()
                    && (parent.kind() == "directive" || parent.kind() == "directive_definition")
                    && let Some(name_node) = self.find_child_by_kind(parent, "name")
                {
                    return Some(format!("@{}", self.get_node_text(name_node, offset)));
                }
            }
        }
        None
    }

    pub fn find_containing_operation_node(
        &self,
        position: Position,
    ) -> Option<(tree_sitter::Node<'_>, usize)> {
        let byte_offset = self.position_to_byte(position);
        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();
            let tree_len = root.end_byte();
            if byte_offset >= offset && byte_offset < offset + tree_len {
                let local_byte = byte_offset - offset;
                let node = root.descendant_for_byte_range(local_byte, local_byte)?;
                let mut curr = node;
                while let Some(parent) = curr.parent() {
                    if parent.kind() == "operation_definition"
                        || parent.kind() == "fragment_definition"
                    {
                        return Some((parent, offset));
                    }
                    curr = parent;
                }
            }
        }
        None
    }

    pub fn find_parent_type_for_node(
        &self,
        node: Node,
        offset: usize,
        schema: &Schema,
    ) -> Option<apollo_compiler::schema::ExtendedType> {
        let mut path = Vec::new();
        let mut curr = node;

        // Collect ancestors that define or change the current type. We intentionally do NOT
        // include the starting node itself here — callers that pass a child (eg. `name`) will
        // have their parent (eg. `field`) included by this traversal; callers that pass a
        // `field` node directly expect the parent type (the type that contains the field) to
        // be returned, so including the starting `field` node would incorrectly descend into
        // the field's own type. This matches historical behavior.
        while let Some(parent) = curr.parent() {
            match parent.kind() {
                // Also include selection and selection_set so we can handle parser-produced ERROR nodes
                // that live directly under a selection_set when the inline fragment is incomplete.
                "field"
                | "inline_fragment"
                | "operation_definition"
                | "fragment_definition"
                | "selection_set"
                | "selection" => {
                    path.push((parent.kind(), parent));
                }
                _ => {}
            }
            curr = parent;
        }

        path.reverse();

        let mut current_type = None;

        for (kind, node) in path {
            match kind {
                "operation_definition" => {
                    let op_type_str = self.get_operation_type(node, offset);
                    let op = match op_type_str.as_str() {
                        "mutation" => apollo_compiler::ast::OperationType::Mutation,
                        "subscription" => apollo_compiler::ast::OperationType::Subscription,
                        _ => apollo_compiler::ast::OperationType::Query,
                    };
                    let root_name = schema
                        .root_operation(op)
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "Query".to_string());
                    current_type = schema.types.get(root_name.as_str()).cloned();
                }
                "fragment_definition" => {
                    if let Some(type_name) = self.get_fragment_type_condition(node, offset) {
                        current_type = schema.types.get(type_name.as_str()).cloned();
                    }
                }
                "field" => {
                    if let Some(parent_type) = current_type.clone()
                        && let Some(field_name_node) = self.extract_field_components(node).name
                    {
                        let field_name = self.get_node_text(field_name_node, offset);
                        let field_def = match &parent_type {
                            ExtendedType::Object(obj) => obj.fields.get(field_name.as_str()),
                            ExtendedType::Interface(iface) => iface.fields.get(field_name.as_str()),
                            _ => None,
                        };
                        if let Some(field_def) = field_def {
                            current_type = schema
                                .types
                                .get(field_def.ty.inner_named_type().as_str())
                                .cloned();
                        } else {
                            current_type = None;
                        }
                    }
                }
                "selection_set" => {
                    if let Some(parent) = node.parent() {
                        match parent.kind() {
                            "field" => {
                                if let Some(parent_type) = current_type.clone()
                                    && let Some(field_name) = self
                                        .extract_field_components(parent)
                                        .name
                                        .map(|n| self.get_node_text(n, offset))
                                    && let Some(field_def) = match &parent_type {
                                        ExtendedType::Object(obj) => {
                                            obj.fields.get(field_name.as_str())
                                        }
                                        ExtendedType::Interface(iface) => {
                                            iface.fields.get(field_name.as_str())
                                        }
                                        _ => None,
                                    }
                                {
                                    current_type = schema
                                        .types
                                        .get(field_def.ty.inner_named_type().as_str())
                                        .cloned();
                                }
                            }
                            "inline_fragment" => {
                                if let Some(type_name) =
                                    self.get_fragment_type_condition(parent, offset)
                                {
                                    current_type = schema.types.get(type_name.as_str()).cloned();
                                }
                            }
                            _ => {}
                        }
                    }
                }
                "selection" => {
                    let mut walker = node.walk();
                    for child in node.children(&mut walker) {
                        if child.kind() == "inline_fragment"
                            && let Some(type_name) = self.get_fragment_type_condition(child, offset)
                        {
                            current_type = schema.types.get(type_name.as_str()).cloned();
                        }
                    }
                }
                "inline_fragment" => {
                    if let Some(type_name) = self.get_fragment_type_condition(node, offset) {
                        current_type = schema.types.get(type_name.as_str()).cloned();
                    }
                }
                _ => {}
            }
        }

        current_type
    }

    pub fn get_fragment_variable_types(
        &self,
        fragment_name: &str,
        schema: &Schema,
    ) -> std::collections::BTreeMap<String, String> {
        let mut vars = std::collections::BTreeMap::new();

        let query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
        });
        let mut cursor = tree_sitter::QueryCursor::new();

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let mut matches = cursor.matches(query, block.tree.root_node(), |node: Node| {
                let start = node.start_byte();
                let end = node.end_byte();
                self.rope
                    .byte_slice((start + offset)..(end + offset))
                    .chunks()
            });

            while let Some(m) = matches.next() {
                let mut name = None;
                let mut is_fragment = false;
                let mut container_node = None;

                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    if cap_name == "symbol.name" {
                        name = Some(self.get_node_text(cap.node, offset));
                    } else if cap_name == "symbol.container"
                        && cap.node.kind() == "fragment_definition"
                    {
                        is_fragment = true;
                        container_node = Some(cap.node);
                    }
                }

                if is_fragment
                    && let Some(n) = name
                    && n == fragment_name
                    && let Some(container) = container_node
                    && let Some(type_name) = self.get_fragment_type_condition(container, offset)
                    && let Some(type_def) = schema.types.get(type_name.as_str())
                {
                    self.collect_variables_in_fragment(
                        container, offset, type_def, schema, &mut vars,
                    );
                }
            }
        }

        vars
    }

    fn collect_variables_in_fragment(
        &self,
        node: Node,
        offset: usize,
        current_type: &apollo_compiler::schema::ExtendedType,
        schema: &Schema,
        vars: &mut std::collections::BTreeMap<String, String>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "selection_set" => {
                    self.collect_variables_in_selection_set(
                        child,
                        offset,
                        current_type,
                        schema,
                        vars,
                    );
                }
                "directives" => {
                    self.collect_variables_in_directives(child, offset, schema, vars);
                }
                "directive" => {
                    self.collect_variables_in_directives(node, offset, schema, vars);
                }
                _ => {
                    self.collect_variables_in_fragment(child, offset, current_type, schema, vars);
                }
            }
        }
    }

    fn collect_variables_in_selection_set(
        &self,
        node: Node,
        offset: usize,
        current_type: &apollo_compiler::schema::ExtendedType,
        schema: &Schema,
        vars: &mut std::collections::BTreeMap<String, String>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "field" => {
                    let mut field_name = None;
                    let mut arguments = None;
                    let mut selection_set = None;
                    let mut directives = None;

                    let mut f_cursor = child.walk();
                    for f_child in child.children(&mut f_cursor) {
                        match f_child.kind() {
                            "name" => field_name = Some(self.get_node_text(f_child, offset)),
                            "arguments" => arguments = Some(f_child),
                            "selection_set" => selection_set = Some(f_child),
                            "directives" => directives = Some(f_child),
                            "directive" => {
                                self.collect_variables_in_directives(child, offset, schema, vars);
                            }
                            _ => {}
                        }
                    }

                    if let Some(fname) = field_name {
                        let field_def = match current_type {
                            apollo_compiler::schema::ExtendedType::Object(obj) => {
                                obj.fields.get(fname.as_str())
                            }
                            apollo_compiler::schema::ExtendedType::Interface(iface) => {
                                iface.fields.get(fname.as_str())
                            }
                            _ => None,
                        };

                        if let Some(fdef) = field_def {
                            if let Some(args_node) = arguments {
                                self.collect_variables_in_arguments(
                                    args_node,
                                    offset,
                                    &fdef.arguments,
                                    schema,
                                    vars,
                                );
                            }

                            if let Some(dirs_node) = directives {
                                self.collect_variables_in_directives(
                                    dirs_node, offset, schema, vars,
                                );
                            }

                            if let Some(sel_node) = selection_set {
                                let next_type_name = fdef.ty.inner_named_type();
                                if let Some(next_type) = schema.types.get(next_type_name.as_str()) {
                                    self.collect_variables_in_selection_set(
                                        sel_node, offset, next_type, schema, vars,
                                    );
                                }
                            }
                        }
                    }
                }
                "inline_fragment" => {
                    let type_name = self.get_fragment_type_condition(child, offset);
                    let target_type = if let Some(tn) = type_name {
                        schema.types.get(tn.as_str()).cloned()
                    } else {
                        Some(current_type.clone())
                    };

                    if let Some(tty) = target_type {
                        self.collect_variables_in_fragment(child, offset, &tty, schema, vars);
                    }
                }
                "selection" => {
                    self.collect_variables_in_selection_set(
                        child,
                        offset,
                        current_type,
                        schema,
                        vars,
                    );
                }
                _ => {}
            }
        }
    }

    fn collect_variables_in_arguments(
        &self,
        node: Node,
        offset: usize,
        arg_defs: &[apollo_compiler::Node<apollo_compiler::schema::InputValueDefinition>],
        schema: &Schema,
        vars: &mut std::collections::BTreeMap<String, String>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "argument" {
                let arg_name = self
                    .find_child_by_kind(child, "name")
                    .map(|n| self.get_node_text(n, offset));

                // Find value child (can be "value" or any kind ending with "_value")
                let mut a_cursor = child.walk();
                let value_node = child
                    .children(&mut a_cursor)
                    .find(|n| n.kind() == "value" || n.kind().ends_with("_value"));

                if let (Some(aname), Some(vnode)) = (arg_name, value_node)
                    && let Some(adef) = arg_defs.iter().find(|a| a.name.as_str() == aname)
                {
                    self.collect_variables_in_value(vnode, offset, &adef.ty, schema, vars);
                }
            }
        }
    }

    fn collect_variables_in_directives(
        &self,
        node: Node,
        offset: usize,
        schema: &Schema,
        vars: &mut std::collections::BTreeMap<String, String>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "directive" {
                let dir_name = self
                    .find_child_by_kind(child, "name")
                    .map(|n| self.get_node_text(n, offset));
                let arguments = self.find_child_by_kind(child, "arguments");

                if let Some(dname) = dir_name
                    && let Some(ddef) = schema.directive_definitions.get(dname.as_str())
                    && let Some(args_node) = arguments
                {
                    self.collect_variables_in_arguments(
                        args_node,
                        offset,
                        &ddef.arguments,
                        schema,
                        vars,
                    );
                }
            }
        }
    }

    fn collect_variables_in_value(
        &self,
        node: Node,
        offset: usize,
        expected_type: &apollo_compiler::schema::Type,
        schema: &Schema,
        vars: &mut std::collections::BTreeMap<String, String>,
    ) {
        match node.kind() {
            "variable" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "name" {
                        let name = self.get_node_text(child, offset);
                        vars.insert(name, expected_type.to_string());
                    }
                }
            }
            "object_value" => {
                if let Some(ty_def) = schema.types.get(expected_type.inner_named_type().as_str())
                    && let apollo_compiler::schema::ExtendedType::InputObject(input_obj) = ty_def
                {
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == "object_field" {
                            let mut field_name = None;
                            let mut value_node = None;
                            let mut of_cursor = child.walk();
                            for of_child in child.children(&mut of_cursor) {
                                if of_child.kind() == "name" {
                                    field_name = Some(self.get_node_text(of_child, offset));
                                } else if of_child.kind() == "value"
                                    || of_child.kind().ends_with("_value")
                                {
                                    value_node = Some(of_child);
                                }
                            }

                            if let (Some(fname), Some(vnode)) = (field_name, value_node)
                                && let Some(fdef) = input_obj.fields.get(fname.as_str())
                            {
                                self.collect_variables_in_value(
                                    vnode, offset, &fdef.ty, schema, vars,
                                );
                            }
                        }
                    }
                }
            }
            "list_value" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "value" || child.kind().ends_with("_value") {
                        self.collect_variables_in_value(child, offset, expected_type, schema, vars);
                    }
                }
            }
            "value" => {
                if let Some(child) = node.child(0) {
                    self.collect_variables_in_value(child, offset, expected_type, schema, vars);
                }
            }
            _ => {}
        }
    }

    pub fn get_completion_context(
        &self,
        position: Position,
        schema: &apollo_compiler::Schema,
    ) -> CompletionContext {
        let offset = self.position_to_byte(position);
        for block in self.get_graphql_trees() {
            if self.is_cursor_in_node_range(block.tree.root_node(), block.offset, offset) {
                let local_byte = offset.saturating_sub(block.offset);
                if let Some(current) = block
                    .tree
                    .root_node()
                    .descendant_for_byte_range(local_byte.saturating_sub(1), local_byte)
                {
                    // Check if we are inside a selection set
                    if let Some(selection_set) =
                        self.find_ancestor_by_kind(current, "selection_set")
                        && let Some(parent_type) =
                            self.find_parent_type_for_node(selection_set, block.offset, schema)
                    {
                        return CompletionContext::SelectionSet(parent_type);
                    }
                }
            }
        }
        CompletionContext::Other
    }
}

#[derive(Debug, Clone)]
pub enum CompletionContext {
    SelectionSet(apollo_compiler::schema::ExtendedType),
    OperationDefinition,
    SchemaDefinition,
    FieldAlias,
    DirectiveArguments,
    UnionMembers,
    ImplementsClause,
    VariableDefaultValue,
    ArgumentDefaultValue,
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn create_doc(src: &str, encoding: PositionEncodingKind) -> DocumentState {
        let uri = Url::parse("file:///tmp/test.tsx").unwrap();
        let mut parser = Parser::new();
        let lang: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TSX.into();
        parser.set_language(&lang).unwrap();
        DocumentState::new(uri, src, parser, encoding)
    }

    #[test]
    fn test_position_encoding_utf8() {
        // "😀" is 4 bytes in UTF-8
        let src = "😀\nnext";
        let doc = create_doc(src, PositionEncodingKind::UTF8);

        // Position of 'n' in "next"
        let pos = doc.byte_to_position(5);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 0);

        let byte = doc.position_to_byte(Position::new(1, 0));
        assert_eq!(byte, 5);

        // Position of the emoji itself
        let pos_emoji = doc.byte_to_position(0);
        assert_eq!(pos_emoji.line, 0);
        assert_eq!(pos_emoji.character, 0);

        // Position after the emoji
        let pos_after_emoji = doc.byte_to_position(4);
        assert_eq!(pos_after_emoji.line, 0);
        assert_eq!(pos_after_emoji.character, 4);
    }

    #[test]
    fn test_position_encoding_utf16() {
        // "😀" is 2 code units in UTF-16 (surrogate pair)
        let src = "😀\nnext";
        let doc = create_doc(src, PositionEncodingKind::UTF16);

        // Position of 'n' in "next"
        let pos = doc.byte_to_position(5);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 0);

        let byte = doc.position_to_byte(Position::new(1, 0));
        assert_eq!(byte, 5);

        // Position after the emoji
        let pos_after_emoji = doc.byte_to_position(4);
        assert_eq!(pos_after_emoji.line, 0);
        assert_eq!(pos_after_emoji.character, 2);
    }

    #[test]
    fn test_position_encoding_utf32() {
        // "😀" is 1 code unit in UTF-32
        let src = "😀\nnext";
        let doc = create_doc(src, PositionEncodingKind::UTF32);

        // Position of 'n' in "next"
        let pos = doc.byte_to_position(5);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.character, 0);

        let byte = doc.position_to_byte(Position::new(1, 0));
        assert_eq!(byte, 5);

        // Position after the emoji
        let pos_after_emoji = doc.byte_to_position(4);
        assert_eq!(pos_after_emoji.line, 0);
        assert_eq!(pos_after_emoji.character, 1);
    }
}
