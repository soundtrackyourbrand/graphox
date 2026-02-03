use crate::queries::*;
use crate::utils::{find_package_root, mask_interpolations};
use ropey::Rope;
use std::path::PathBuf;
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

pub struct GraphQLBlock {
    pub tree: Tree,
    pub offset: usize,
}

#[derive(Debug, Clone)]
pub struct FragmentDef {
    pub name: String,
    pub is_public: bool,
}

pub struct DocumentState {
    pub uri: Url,
    pub rope: Rope,
    pub tree: Tree,
    pub language: DocumentLanguage,
    pub graphql_trees: Vec<GraphQLBlock>,
    pub fragments: Vec<FragmentDef>,
    pub package_root: Option<PathBuf>,
}

impl DocumentState {
    pub fn new(uri: Url, text: &str, mut parser: tree_sitter::Parser) -> Self {
        let language = DocumentLanguage::from_uri(&uri);
        let rope = Rope::from_str(text);
        let tree = parser.parse(text, None).unwrap();
        let package_root = if let Ok(path) = uri.to_file_path() {
            find_package_root(&path)
        } else {
            None
        };

        let mut doc = Self {
            uri,
            rope,
            tree,
            language,
            graphql_trees: Vec::new(),
            fragments: Vec::new(),
            package_root,
        };
        doc.graphql_trees = doc.reparse_graphql_trees();
        doc.fragments = doc.extract_fragment_names();
        doc
    }

    pub fn reparse_graphql_trees(&self) -> Vec<GraphQLBlock> {
        if self.language == DocumentLanguage::GraphQL {
            return vec![GraphQLBlock {
                tree: self.tree.clone(),
                offset: 0,
            }];
        }

        // TypeScript/TSX handling - Fast check first
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
        let mut seen_nodes = std::collections::HashSet::new();

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
                    // Check for comment more robustly
                    let mut curr = node.prev_named_sibling();
                    while let Some(prev) = curr {
                        if prev.kind() == "comment" {
                            let text = self.get_node_text(prev, 0);
                            if text.to_uppercase().contains("GRAPHQL") {
                                gql_node = Some(node);
                                break;
                            }
                        }
                        // Skip other named nodes if they might be between comment and template
                        // but usually in TS they are direct siblings in arguments or declarators.
                        // We only want to go back a little bit.
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
                        tree: gql_tree,
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

        // Ropey methods are char_to_utf16_cu
        let utf16_cu_at_offset = self.rope.char_to_utf16_cu(char_at_offset);
        let utf16_cu_at_line_start = self.rope.char_to_utf16_cu(char_at_line_start);

        Position::new(
            line as u32,
            (utf16_cu_at_offset - utf16_cu_at_line_start) as u32,
        )
    }

    pub fn position_to_byte(&self, position: Position) -> usize {
        let line_idx = position.line as usize;
        let line_start_char = self.rope.line_to_char(line_idx);
        let line_start_utf16_cu = self.rope.char_to_utf16_cu(line_start_char);

        let target_utf16_cu = line_start_utf16_cu + position.character as usize;
        let target_char = self.rope.utf16_cu_to_char(target_utf16_cu);
        self.rope.char_to_byte(target_char)
    }

    pub fn has_graphql_candidates(&self) -> bool {
        if self.language == DocumentLanguage::GraphQL {
            return true;
        }
        self.contains_ignore_case("GQL") || self.contains_ignore_case("GRAPHQL")
    }

    fn contains_ignore_case(&self, needle: &str) -> bool {
        let needle_upper = needle.to_uppercase();
        let mut full_text = String::new();
        for chunk in self.rope.chunks() {
            full_text.push_str(chunk);
        }
        full_text.to_uppercase().contains(&needle_upper)
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
                let mut is_fragment = false;
                let mut is_public = false;

                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    if cap_name == "symbol.name" {
                        name = Some(self.get_node_text(cap.node, offset));
                    } else if cap_name == "symbol.container" {
                        if cap.node.kind() == "fragment_definition" {
                            is_fragment = true;
                        }
                    } else if cap_name == "symbol.directives" {
                        let directives_text = self.get_node_text(cap.node, offset);
                        if directives_text.contains("@public") {
                            is_public = true;
                        }
                    }
                }

                if is_fragment && let Some(n) = name {
                    fragments.push(FragmentDef { name: n, is_public });
                }
            }
        }
        fragments
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
                        container_node = Some(cap.node);
                        if cap.node.kind() == "fragment_definition" {
                            is_fragment = true;
                        }
                    }
                }

                if is_fragment
                    && let Some(n) = name
                    && n == target_name
                    && let Some(cont) = container_node
                {
                    // Extract the selection set or the whole fragment
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
                start_position: Point::new(range.start.line as usize, range.start.character as usize),
                old_end_position: Point::new(range.end.line as usize, range.end.character as usize),
                new_end_position: Point::new(new_end_line, new_end_col_utf16),
            };

            self.tree.edit(&edit);

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
        } else {
            // Full update
            self.rope = Rope::from_str(&change.text);
            self.tree = parser.parse(&change.text, None).unwrap();
        }

        self.graphql_trees = self.reparse_graphql_trees();
        self.fragments = self.extract_fragment_names();
    }
}
