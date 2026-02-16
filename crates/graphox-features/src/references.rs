use apollo_compiler::Schema;
use graphox_core::document::DocumentState;
use graphox_core::queries::*;
use lsp_types::*;
use tree_sitter::{QueryCursor, StreamingIterator};

pub trait DocumentReferences {
    fn find_variable_references(
        &self,
        symbol_name: &str,
        position: Position,
        include_declaration: bool,
    ) -> Vec<Location>;

    fn find_references_in_tree(
        &self,
        target_name: &str,
        include_declaration: bool,
    ) -> Vec<Location>;

    fn find_field_references(
        &self,
        field_name: &str,
        parent_type_name: &str,
        schema: &Schema,
        include_declaration: bool,
    ) -> Vec<Location>;

    fn find_directive_references(
        &self,
        directive_name: &str,
        include_declaration: bool,
    ) -> Vec<Location>;
}

impl DocumentReferences for DocumentState {
    fn find_variable_references(
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

    fn find_references_in_tree(
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

    fn find_field_references(
        &self,
        field_name: &str,
        parent_type_name: &str,
        schema: &Schema,
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

                    if name == field_name {
                        // For definitions, we need to check if this is specifically a field_definition
                        if is_definition {
                            // Check if the parent is field_definition
                            let Some(parent) = name_node.parent() else {
                                continue;
                            };
                            if parent.kind() != "field_definition" {
                                // Not a field definition, skip it
                                continue;
                            }

                            if let Some(resolved_type) =
                                find_ancestor_type_for_field_def(self, name_node, offset)
                                && resolved_type == parent_type_name
                            {
                                let range = self.translate_to_file_range(name_node, offset);
                                let key = (range.start.line, range.start.character);
                                definitions.insert(key);
                                results_by_range.insert(
                                    key,
                                    Location {
                                        uri: self.uri.clone(),
                                        range,
                                    },
                                );
                            }
                        } else {
                            // For references, check if this is a field selection
                            let Some(parent) = name_node.parent() else {
                                continue;
                            };
                            if parent.kind() != "field" {
                                // Not a field selection, skip it
                                continue;
                            }

                            if is_field_on_type(self, name_node, offset, parent_type_name, schema) {
                                let range = self.translate_to_file_range(name_node, offset);
                                let key = (range.start.line, range.start.character);
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
            }
        }

        results_by_range
            .into_iter()
            .filter(|(key, _)| include_declaration || !definitions.contains(key))
            .map(|(_, loc)| loc)
            .collect()
    }

    fn find_directive_references(
        &self,
        directive_name: &str,
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
                let mut is_directive_node = false;
                let mut name_node = None;

                for cap in m.captures {
                    if cap.index == definition_idx {
                        let node = cap.node;
                        if node.kind() == "directive_definition" {
                            is_directive_node = true;
                        }
                        is_definition = true;
                    } else if cap.index == reference_idx {
                        let node = cap.node;
                        if node.kind() == "directive" {
                            is_directive_node = true;
                        }
                    } else {
                        name_node = Some(cap.node);
                    }
                }

                if !is_directive_node {
                    continue;
                }

                if let Some(name_node) = name_node {
                    let name = self
                        .rope
                        .slice(
                            self.rope.byte_to_char(name_node.start_byte() + offset)
                                ..self.rope.byte_to_char(name_node.end_byte() + offset),
                        )
                        .to_string();

                    if name == directive_name {
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

/// Find the parent type name for a field_definition node in a schema.
fn find_ancestor_type_for_field_def(
    doc: &DocumentState,
    node: tree_sitter::Node,
    offset: usize,
) -> Option<String> {
    let mut curr = node;
    while let Some(parent) = curr.parent() {
        match parent.kind() {
            "object_type_definition" | "interface_type_definition" => {
                // Find the name child of the type definition
                if let Some(name_node) = doc.find_child_by_kind(parent, "name") {
                    return Some(doc.get_node_text(name_node, offset));
                }
            }
            _ => {
                curr = parent;
            }
        }
    }
    None
}

/// Check if a field selection node is selecting a field on the given parent type.
fn is_field_on_type(
    doc: &DocumentState,
    name_node: tree_sitter::Node,
    offset: usize,
    expected_parent_type: &str,
    schema: &Schema,
) -> bool {
    // Find the field node parent
    let Some(field_node) = name_node.parent() else {
        return false;
    };
    if field_node.kind() != "field" {
        return false;
    }

    // Use find_parent_type_for_node to resolve the actual parent type
    let Some(actual_parent_type) = doc.find_parent_type_for_node(field_node, offset, schema) else {
        return false;
    };

    actual_parent_type.name() == expected_parent_type
}
