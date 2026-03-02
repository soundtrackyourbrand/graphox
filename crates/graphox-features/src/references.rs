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

    fn find_variable_declaration(&self, name: &str, position: Position) -> Option<Location>;

    fn find_enum_value_references(
        &self,
        enum_name: &str,
        enum_value_name: &str,
        schema: &apollo_compiler::Schema,
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

            let mut results_by_range = ahash::AHashMap::default();
            let mut definitions = ahash::AHashSet::default();

            let mut cursor = QueryCursor::new();
            let mut matches = cursor.matches(query, op_node, |node: tree_sitter::Node| {
                let start = node.start_byte();
                let end = node.end_byte();
                self.rope
                    .byte_slice((start + offset)..(end + offset))
                    .chunks()
            });

            while let Some(m) = matches.next() {
                let mut is_def = false;
                let mut name_node = None;

                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    match cap_name {
                        "definition" => is_def = true,
                        "name" => name_node = Some(cap.node),
                        _ => {}
                    }
                }

                if let Some(node) = name_node {
                    let text = self.get_node_text(node, offset);
                    if text == symbol_name {
                        let range = self.translate_to_file_range(node, offset);
                        let range_key = (
                            range.start.line,
                            range.start.character,
                            range.end.line,
                            range.end.character,
                        );

                        results_by_range.insert(
                            range_key,
                            Location {
                                uri: self.uri.clone(),
                                range,
                            },
                        );

                        if is_def {
                            definitions.insert(range_key);
                        }
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

        let mut results_by_range = ahash::AHashMap::default();
        let mut definitions = ahash::AHashSet::default();

        let mut cursor = QueryCursor::new();
        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let mut matches =
                cursor.matches(query, block.tree.root_node(), |n: tree_sitter::Node| {
                    let start = n.start_byte();
                    let end = n.end_byte();
                    self.rope
                        .byte_slice((start + offset)..(end + offset))
                        .chunks()
                });

            while let Some(m) = matches.next() {
                let mut is_def = false;
                let mut name_node = None;

                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    match cap_name {
                        "definition" => is_def = true,
                        "name" => name_node = Some(cap.node),
                        _ => {}
                    }
                }

                if let Some(node) = name_node {
                    let text = self.get_node_text(node, offset);
                    if text == target_name {
                        let range = self.translate_to_file_range(node, offset);
                        let range_key = (
                            range.start.line,
                            range.start.character,
                            range.end.line,
                            range.end.character,
                        );

                        results_by_range.insert(
                            range_key,
                            Location {
                                uri: self.uri.clone(),
                                range,
                            },
                        );

                        if is_def {
                            definitions.insert(range_key);
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

    fn find_field_references(
        &self,
        field_name: &str,
        parent_type_name: &str,
        schema: &Schema,
        include_declaration: bool,
    ) -> Vec<Location> {
        let mut results_by_range = ahash::AHashMap::default();
        let mut definitions = ahash::AHashSet::default();

        let query = GQL_REFERENCES_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_REFERENCES_QUERY).unwrap()
        });

        let mut cursor = QueryCursor::new();
        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let mut matches =
                cursor.matches(query, block.tree.root_node(), |n: tree_sitter::Node| {
                    let start = n.start_byte();
                    let end = n.end_byte();
                    self.rope
                        .byte_slice((start + offset)..(end + offset))
                        .chunks()
                });

            while let Some(m) = matches.next() {
                let mut is_def = false;
                let mut name_node = None;

                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    match cap_name {
                        "definition" => is_def = true,
                        "name" => name_node = Some(cap.node),
                        _ => {}
                    }
                }

                if let Some(node) = name_node {
                    let text = self.get_node_text(node, offset);
                    if text == field_name {
                        // Use parent node (eg. 'field') to get the type that CONTAINS this field
                        let search_node = if let Some(parent) = node.parent() {
                            parent
                        } else {
                            node
                        };

                        // Check if parent type matches
                        if let Some(actual_parent_type) =
                            self.find_parent_type_for_node(search_node, offset, schema)
                            && (parent_type_name.is_empty()
                                || actual_parent_type.name() == parent_type_name)
                        {
                            let range = self.translate_to_file_range(node, offset);
                            let range_key = (
                                range.start.line,
                                range.start.character,
                                range.end.line,
                                range.end.character,
                            );

                            results_by_range.insert(
                                range_key,
                                Location {
                                    uri: self.uri.clone(),
                                    range,
                                },
                            );

                            if is_def {
                                definitions.insert(range_key);
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
        let directive_name = directive_name.strip_prefix('@').unwrap_or(directive_name);
        let mut results_by_range = ahash::AHashMap::default();
        let mut definitions = ahash::AHashSet::default();

        let query = GQL_REFERENCES_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_REFERENCES_QUERY).unwrap()
        });

        let mut cursor = QueryCursor::new();
        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let mut matches =
                cursor.matches(query, block.tree.root_node(), |n: tree_sitter::Node| {
                    let start = n.start_byte();
                    let end = n.end_byte();
                    self.rope
                        .byte_slice((start + offset)..(end + offset))
                        .chunks()
                });

            while let Some(m) = matches.next() {
                let mut is_def = false;
                let mut name_node = None;

                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    match cap_name {
                        "definition" => is_def = true,
                        "name" => name_node = Some(cap.node),
                        _ => {}
                    }
                }

                if let Some(node) = name_node {
                    let text = self.get_node_text(node, offset);
                    if text == directive_name
                        && let Some(parent) = node.parent()
                        && (parent.kind() == "directive" || parent.kind() == "directive_definition")
                    {
                        let range = self.translate_to_file_range(node, offset);
                        let range_key = (
                            range.start.line,
                            range.start.character,
                            range.end.line,
                            range.end.character,
                        );

                        results_by_range.insert(
                            range_key,
                            Location {
                                uri: self.uri.clone(),
                                range,
                            },
                        );

                        if is_def {
                            definitions.insert(range_key);
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

    fn find_variable_declaration(&self, name: &str, position: Position) -> Option<Location> {
        if !name.starts_with('$') {
            return None;
        }

        if let Some((op_node, offset)) = self.find_containing_operation_node(position) {
            let mut cursor = op_node.walk();
            for node in op_node.children(&mut cursor) {
                if node.kind() == "variable_definition"
                    && let Some(v) = self.find_child_by_kind(node, "variable")
                    && let Some(n) = self.find_child_by_kind(v, "name")
                    && self.get_node_text(n, offset) == name
                {
                    return Some(Location {
                        uri: self.uri.clone(),
                        range: self.translate_to_file_range(v, offset),
                    });
                }
            }
        }
        None
    }

    fn find_enum_value_references(
        &self,
        enum_name: &str,
        enum_value_name: &str,
        schema: &Schema,
        include_declaration: bool,
    ) -> Vec<Location> {
        let mut results_by_range = ahash::AHashMap::default();
        let mut definitions = ahash::AHashSet::default();

        let query = GQL_REFERENCES_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_REFERENCES_QUERY).unwrap()
        });

        let mut cursor = QueryCursor::new();
        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let mut matches =
                cursor.matches(query, block.tree.root_node(), |n: tree_sitter::Node| {
                    let start = n.start_byte();
                    let end = n.end_byte();
                    self.rope
                        .byte_slice((start + offset)..(end + offset))
                        .chunks()
                });

            while let Some(m) = matches.next() {
                let mut is_def = false;
                let mut name_node = None;

                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    match cap_name {
                        "definition" => is_def = true,
                        "name" => name_node = Some(cap.node),
                        _ => {}
                    }
                }

                if let Some(node) = name_node {
                    let text = self.get_node_text(node, offset);
                    if text == enum_value_name {
                        let mut is_match = false;
                        if is_def {
                            if let Some(p) = self.find_ancestor_by_kinds(
                                node,
                                &["enum_type_definition", "enum_type_extension"],
                            ) && let Some(n) = self.find_child_by_kind(p, "name")
                                && self.get_node_text(n, offset) == enum_name
                            {
                                is_match = true;
                            }
                        } else if let Some(actual_type) =
                            self.find_parent_type_for_node(node, offset, schema)
                            && actual_type.name() == enum_name
                        {
                            is_match = true;
                        }
                        if is_match {
                            let range = self.translate_to_file_range(node, offset);
                            let range_key = (
                                range.start.line,
                                range.start.character,
                                range.end.line,
                                range.end.character,
                            );

                            results_by_range.insert(
                                range_key,
                                Location {
                                    uri: self.uri.clone(),
                                    range,
                                },
                            );

                            if is_def {
                                definitions.insert(range_key);
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
}
