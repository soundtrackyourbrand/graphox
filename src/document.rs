use crate::queries::*;
use crate::utils::{find_package_root, mask_interpolations};
use apollo_compiler::Schema;
use ropey::Rope;
use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::lsp_types::*;
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
    pub name: String,
    pub type_condition: String,
    pub is_public: bool,
    pub description: Option<String>,
    pub source_hash: u64,
    pub used_variables: Vec<String>,
    pub used_fragments: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct OperationDef {
    pub name: Option<String>,
    pub operation_type: String,
    pub source_text: String,
}

#[derive(Clone)]
pub struct DocumentState {
    pub uri: Url,
    pub rope: Rope,
    pub tree: Arc<Tree>,
    pub language: DocumentLanguage,
    pub graphql_trees: Vec<GraphQLBlock>,
    pub fragments: Vec<FragmentDef>,
    pub fragment_spreads: Vec<String>,
    pub operations: Vec<OperationDef>,
    pub package_root: Option<PathBuf>,
    pub masked_source: String,
}

impl fmt::Debug for DocumentState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DocumentState")
            .field("uri", &self.uri)
            .field("language", &self.language)
            .finish()
    }
}

impl DocumentState {
    pub fn new(uri: Url, text: &str, mut parser: tree_sitter::Parser) -> Self {
        let language = DocumentLanguage::from_uri(&uri);
        let rope = Rope::from_str(text);
        let tree = Arc::new(parser.parse(text, None).unwrap());
        let package_root = if let Ok(path) = uri.to_file_path() {
            find_package_root(&path)
        } else {
            None
        };

        let masked_source = if language.is_host_language() {
            mask_interpolations(text)
        } else {
            text.to_string()
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
        };
        doc.graphql_trees = doc.reparse_graphql_trees();
        doc.fragments = doc.extract_fragment_names();
        doc.fragment_spreads = doc.extract_fragment_spreads();
        doc.operations = doc.extract_operations();
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
        let mut seen_nodes = fnv::FnvHashSet::default();

        let mut gql_parser = Parser::new();
        gql_parser
            .set_language(&tree_sitter_graphql::LANGUAGE.into())
            .unwrap();

        while let Some(m) = ts_matches.next() {
            let mut gql_node = None;

            for cap in m.captures {
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

                let start_byte = node.start_byte() + 1;
                let end_byte = node.end_byte() - 1;
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

    fn byte_to_position(&self, byte_offset: usize) -> Position {
        let line = self.rope.byte_to_line(byte_offset);
        let line_start_byte = self.rope.line_to_byte(line);

        let char_at_offset = self.rope.byte_to_char(byte_offset);
        let char_at_line_start = self.rope.byte_to_char(line_start_byte);

        let utf16_cu_at_offset = self.rope.char_to_utf16_cu(char_at_offset);
        let utf16_cu_at_line_start = self.rope.char_to_utf16_cu(char_at_line_start);

        Position::new(
            line as u32,
            (utf16_cu_at_offset - utf16_cu_at_line_start) as u32,
        )
    }

    pub fn position_to_byte(&self, position: Position) -> usize {
        let line_idx = position.line as usize;
        if line_idx >= self.rope.len_lines() {
            return self.rope.len_bytes();
        }
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

    pub fn fragments(&self) -> &[FragmentDef] {
        &self.fragments
    }

    pub fn operations(&self) -> &[OperationDef] {
        &self.operations
    }

    pub fn extract_fragment_names(&self) -> Vec<FragmentDef> {
        let query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
        });

        let mut cursor = tree_sitter::QueryCursor::new();
        let mut fragments = Vec::new();

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
                let mut type_condition = None;
                let mut is_fragment = false;
                let mut is_public = false;
                let mut description = None;
                let mut container_node = None;

                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    if cap_name == "symbol.name" {
                        name = Some(self.get_node_text(cap.node, offset));
                    } else if cap_name == "symbol.type_condition" {
                        type_condition = Some(self.get_node_text(cap.node, offset));
                    } else if cap_name == "symbol.container" {
                        if cap.node.kind() == "fragment_definition" {
                            is_fragment = true;
                            container_node = Some(cap.node);
                        }
                    } else if cap_name == "symbol.directives" {
                        let directives_text = self.get_node_text(cap.node, offset);
                        if directives_text.contains("@public") {
                            is_public = true;
                        }
                    }
                }

                if is_fragment && let Some(n) = name {
                    let mut source_hash = 0;
                    let mut used_variables = Vec::new();
                    let mut used_fragments = Vec::new();
                    if let Some(container) = container_node {
                        let mut hasher = ahash::AHasher::default();
                        use std::hash::{Hash, Hasher};
                        self.get_node_text(container, offset).hash(&mut hasher);
                        source_hash = hasher.finish();

                        let mut walker = container.walk();
                        for child in container.children(&mut walker) {
                            if child.kind() == "description" {
                                if let Some(sv) = child.child_by_field_name("content") {
                                    description = Some(
                                        self.get_node_text(sv, offset)
                                            .trim_matches('"')
                                            .to_string(),
                                    );
                                } else if let Some(sv) = child.child(0) {
                                    description = Some(
                                        self.get_node_text(sv, offset)
                                            .trim_matches('"')
                                            .to_string(),
                                    );
                                }
                            }
                        }

                        let ref_query = GQL_REFERENCES_QUERY_CACHE.get_or_init(|| {
                            let lang = tree_sitter_graphql::LANGUAGE.into();
                            tree_sitter::Query::new(&lang, GQL_REFERENCES_QUERY).unwrap()
                        });
                        let mut ref_cursor = tree_sitter::QueryCursor::new();

                        let mut ref_matches =
                            ref_cursor.matches(ref_query, container, |node: Node| {
                                let start = node.start_byte();
                                let end = node.end_byte();
                                self.rope
                                    .byte_slice((start + offset)..(end + offset))
                                    .chunks()
                            });

                        while let Some(rm) = ref_matches.next() {
                            let mut is_reference = false;
                            let mut name_node = None;
                            for rcap in rm.captures {
                                if ref_query.capture_names()[rcap.index as usize] == "reference" {
                                    is_reference = true;
                                } else if ref_query.capture_names()[rcap.index as usize] == "name" {
                                    name_node = Some(rcap.node);
                                }
                            }

                            if is_reference && let Some(nn) = name_node {
                                let node_text = self.get_node_text(nn, offset);
                                if node_text.starts_with('$') {
                                    let mut v_cursor = nn.walk();
                                    for v_child in nn.children(&mut v_cursor) {
                                        if v_child.kind() == "name" {
                                            used_variables
                                                .push(self.get_node_text(v_child, offset));
                                        }
                                    }
                                } else if let Some(parent) = nn.parent() {
                                    if parent.kind() == "fragment_name" {
                                        used_fragments.push(node_text);
                                    }
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
                                        Some(trimmed.trim_start_matches('#').trim().to_string());
                                }
                            }
                        }
                    }

                    fragments.push(FragmentDef {
                        name: n,
                        type_condition: type_condition.unwrap_or_default(),
                        is_public,
                        description,
                        source_hash,
                        used_variables,
                        used_fragments,
                    });
                }
            }
        }
        fragments
    }

    pub fn extract_fragment_spreads(&self) -> Vec<String> {
        let mut spreads = Vec::new();
        for block in self.get_graphql_trees() {
            spreads.extend(self.get_fragment_spreads_in_node(block.tree.root_node(), block.offset));
        }
        spreads
    }

    pub fn get_fragment_spreads_in_node(&self, node: Node, offset: usize) -> Vec<String> {
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
                if !name.starts_with('$') {
                    if let Some(parent) = name_node.parent() {
                        if parent.kind() == "fragment_name" {
                            spreads.push(name);
                        }
                    }
                }
            }
        }
        spreads
    }

    pub fn extract_operations(&self) -> Vec<OperationDef> {
        let query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
        });

        let mut cursor = tree_sitter::QueryCursor::new();
        let mut operations = Vec::new();

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
                let mut op_type = String::from("query");
                let mut is_operation = false;
                let mut full_node = None;

                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    if cap_name == "symbol.name" {
                        name = Some(self.get_node_text(cap.node, offset));
                    } else if cap_name == "symbol.container" {
                        if cap.node.kind() == "operation_type" {
                            op_type = self.get_node_text(cap.node, offset);
                            is_operation = true;
                        }
                    } else if cap_name == "symbol.full" {
                        if cap.node.kind() == "operation_definition" {
                            is_operation = true;
                            full_node = Some(cap.node);
                        }
                    }
                }

                if is_operation {
                    let source_text = if let Some(n) = full_node {
                        self.get_node_text(n, offset)
                    } else {
                        block
                            .tree
                            .root_node()
                            .utf8_text(b"")
                            .unwrap_or("")
                            .to_string()
                    };

                    operations.push(OperationDef {
                        name,
                        operation_type: op_type,
                        source_text,
                    });
                }
            }
        }
        operations
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
                    } else if cap_name == "symbol.container" {
                        if cap.node.kind() == "fragment_definition" {
                            is_fragment = true;
                            container_node = Some(cap.node);
                        }
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
        parser: &mut tree_sitter::Parser,
    ) {
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
        self.fragments = self.extract_fragment_names();
        self.fragment_spreads = self.extract_fragment_spreads();

        self.masked_source = if self.language.is_host_language() {
            mask_interpolations(&self.rope.to_string())
        } else {
            self.rope.to_string()
        };
    }

    pub fn find_parent_type_for_node(
        &self,
        node: Node,
        offset: usize,
        schema: &Schema,
    ) -> Option<apollo_compiler::schema::ExtendedType> {
        let mut current = node;
        while current.kind() != "selection_set" {
            if let Some(parent) = current.parent() {
                current = parent;
            } else {
                return None;
            }
        }

        let container = current.parent()?;
        match container.kind() {
            "operation_definition" => {
                let mut op_type_str = "query";
                let mut walker = container.walk();
                for child in container.children(&mut walker) {
                    if child.kind() == "operation_type" {
                        let text = self.get_node_text(child, offset);
                        if text == "mutation" {
                            op_type_str = "mutation";
                        } else if text == "subscription" {
                            op_type_str = "subscription";
                        }
                        break;
                    }
                }
                let op = match op_type_str {
                    "mutation" => apollo_compiler::ast::OperationType::Mutation,
                    "subscription" => apollo_compiler::ast::OperationType::Subscription,
                    _ => apollo_compiler::ast::OperationType::Query,
                };
                let root_name = schema
                    .root_operation(op)
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "Query".to_string());
                schema.types.get(root_name.as_str()).cloned()
            }
            "fragment_definition" => {
                let type_name = self.get_fragment_type_condition(container, offset)?;
                schema.types.get(type_name.as_str()).cloned()
            }
            "field" => {
                let parent_of_field = self.find_parent_type_for_node(container, offset, schema)?;
                let mut f_name = None;
                let mut f_walker = container.walk();
                for child in container.children(&mut f_walker) {
                    if child.kind() == "name" {
                        f_name = Some(self.get_node_text(child, offset));
                        break;
                    }
                }
                let field_name = f_name?;
                let field_def = match &parent_of_field {
                    apollo_compiler::schema::ExtendedType::Object(obj) => {
                        obj.fields.get(field_name.as_str())
                    }
                    apollo_compiler::schema::ExtendedType::Interface(iface) => {
                        iface.fields.get(field_name.as_str())
                    }
                    _ => None,
                }?;
                schema
                    .types
                    .get(field_def.ty.inner_named_type().as_str())
                    .cloned()
            }
            "inline_fragment" => {
                if let Some(type_name) = self.get_fragment_type_condition(container, offset) {
                    schema.types.get(type_name.as_str()).cloned()
                } else {
                    self.find_parent_type_for_node(container, offset, schema)
                }
            }
            _ => None,
        }
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
                    } else if cap_name == "symbol.container" {
                        if cap.node.kind() == "fragment_definition" {
                            is_fragment = true;
                            container_node = Some(cap.node);
                        }
                    }
                }

                if is_fragment && let Some(n) = name {
                    if n == fragment_name
                        && let Some(container) = container_node
                    {
                        if let Some(type_name) = self.get_fragment_type_condition(container, offset)
                        {
                            if let Some(type_def) = schema.types.get(type_name.as_str()) {
                                self.collect_variables_in_fragment(
                                    container, offset, type_def, schema, &mut vars,
                                );
                            }
                        }
                    }
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
                let mut arg_name = None;
                let mut value_node = None;
                let mut a_cursor = child.walk();
                for a_child in child.children(&mut a_cursor) {
                    if a_child.kind() == "name" {
                        arg_name = Some(self.get_node_text(a_child, offset));
                    } else if a_child.kind() == "value" || a_child.kind().ends_with("_value") {
                        value_node = Some(a_child);
                    }
                }

                if let (Some(aname), Some(vnode)) = (arg_name, value_node) {
                    if let Some(adef) = arg_defs.iter().find(|a| a.name.as_str() == aname) {
                        self.collect_variables_in_value(vnode, offset, &adef.ty, schema, vars);
                    }
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
                let mut dir_name = None;
                let mut arguments = None;
                let mut d_cursor = child.walk();
                for d_child in child.children(&mut d_cursor) {
                    if d_child.kind() == "name" {
                        dir_name = Some(self.get_node_text(d_child, offset));
                    } else if d_child.kind() == "arguments" {
                        arguments = Some(d_child);
                    }
                }

                if let Some(dname) = dir_name {
                    if let Some(ddef) = schema.directive_definitions.get(dname.as_str()) {
                        if let Some(args_node) = arguments {
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
                if let Some(ty_def) = schema.types.get(expected_type.inner_named_type().as_str()) {
                    if let apollo_compiler::schema::ExtendedType::InputObject(input_obj) = ty_def {
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

                                if let (Some(fname), Some(vnode)) = (field_name, value_node) {
                                    if let Some(fdef) = input_obj.fields.get(fname.as_str()) {
                                        self.collect_variables_in_value(
                                            vnode, offset, &fdef.ty, schema, vars,
                                        );
                                    }
                                }
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
}
