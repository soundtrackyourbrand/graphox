use crate::document::DocumentState;
use crate::queries::*;
use tower_lsp::lsp_types::*;
use tree_sitter::{QueryCursor, StreamingIterator};

impl DocumentState {
    pub fn find_variable_references(
        &self,
        symbol_name: &str,
        position: Position,
        include_declaration: bool,
    ) -> Vec<Location> {
        if !symbol_name.starts_with('$') {
            return Vec::new();
        }

        if let Some((op_node, offset)) = self.find_containing_operation_node(position) {
            let query = GQL_REFERENCES_QUERY_CACHE.get_or_init(|| {
                let lang = tree_sitter_graphql::LANGUAGE.into();
                tree_sitter::Query::new(&lang, GQL_REFERENCES_QUERY).unwrap()
            });

            let mut cursor = QueryCursor::new();
            let mut results_by_range: std::collections::BTreeMap<(u32, u32), Location> =
                std::collections::BTreeMap::new();
            let mut definitions: ahash::AHashSet<(u32, u32)> = ahash::AHashSet::default();

            let reference_idx = query.capture_index_for_name("reference").unwrap();
            let definition_idx = query.capture_index_for_name("definition").unwrap();

            let mut matches = cursor.matches(query, op_node, |node: tree_sitter::Node| {
                let start = node.start_byte();
                let end = node.end_byte();
                self.rope
                    .byte_slice((start + offset)..(end + offset))
                    .chunks()
            });

            while let Some(m) = matches.next() {
                let mut is_definition = false;
                let mut name_node = None;

                for cap in m.captures {
                    if cap.index == definition_idx {
                        is_definition = true;
                    } else if cap.index == reference_idx {
                        // is_definition remains false
                    } else {
                        // This must be the "name" capture
                        name_node = Some(cap.node);
                    }
                }

                if let Some(name_node) = name_node {
                    let name = self
                        .rope
                        .slice(
                            self.rope.byte_to_char(name_node.start_byte() + offset)
                                ..self.rope.byte_to_char(name_node.end_byte() + offset),
                        )
                        .to_string();

                    if name == symbol_name {
                        let range = self.translate_to_file_range(name_node, offset);
                        let key = (range.start.line, range.start.character);

                        if is_definition {
                            definitions.insert(key);
                        }

                        results_by_range.insert(
                            key,
                            Location {
                                uri: self.uri.clone(),
                                range,
                            },
                        );
                    }
                }
            }

            return results_by_range
                .into_iter()
                .filter(|(key, _)| include_declaration || !definitions.contains(key))
                .map(|(_, loc)| loc)
                .collect();
        }

        Vec::new()
    }

    pub fn find_references_in_tree(
        &self,
        target_name: &str,
        include_declaration: bool,
    ) -> Vec<Location> {
        let query = GQL_REFERENCES_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_REFERENCES_QUERY).unwrap()
        });

        let mut cursor = QueryCursor::new();
        let mut results_by_range: std::collections::BTreeMap<(u32, u32), Location> =
            std::collections::BTreeMap::new();
        let mut definitions: ahash::AHashSet<(u32, u32)> = ahash::AHashSet::default();

        let reference_idx = query.capture_index_for_name("reference").unwrap();
        let definition_idx = query.capture_index_for_name("definition").unwrap();

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let mut matches =
                cursor.matches(query, block.tree.root_node(), |node: tree_sitter::Node| {
                    let start = node.start_byte();
                    let end = node.end_byte();
                    self.rope
                        .byte_slice((start + offset)..(end + offset))
                        .chunks()
                });

            while let Some(m) = matches.next() {
                let mut is_definition = false;
                let mut name_node = None;

                for cap in m.captures {
                    if cap.index == definition_idx {
                        is_definition = true;
                    } else if cap.index == reference_idx {
                        // is_definition remains false
                    } else {
                        // This must be the "name" capture
                        name_node = Some(cap.node);
                    }
                }

                if let Some(name_node) = name_node {
                    let name = self
                        .rope
                        .slice(
                            self.rope.byte_to_char(name_node.start_byte() + offset)
                                ..self.rope.byte_to_char(name_node.end_byte() + offset),
                        )
                        .to_string();

                    if name == target_name {
                        let range = self.translate_to_file_range(name_node, offset);
                        let key = (range.start.line, range.start.character);

                        if is_definition {
                            definitions.insert(key);
                        }

                        results_by_range.insert(
                            key,
                            Location {
                                uri: self.uri.clone(),
                                range,
                            },
                        );
                    }
                }
            }
        }

        results_by_range
            .into_iter()
            .filter(|(key, _)| include_declaration || !definitions.contains(key))
            .map(|(_, loc)| loc)
            .collect()
    }
}
