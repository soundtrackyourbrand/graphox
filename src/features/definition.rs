use crate::document::DocumentState;
use crate::queries::*;
use tower_lsp::lsp_types::*;
use tree_sitter::{QueryCursor, StreamingIterator};

impl DocumentState {
    pub fn get_symbol_at_position(&self, position: Position) -> Option<String> {
        let byte_offset = self.position_to_byte(position);

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();
            let tree_len = root.end_byte();

            if byte_offset >= offset && byte_offset < offset + tree_len {
                let local_byte = byte_offset - offset;
                let trigger_node = root.descendant_for_byte_range(local_byte, local_byte)?;

                let mut target_node = trigger_node;
                if target_node.kind() == "name"
                    && let Some(parent) = target_node.parent()
                    && parent.kind() == "variable"
                {
                    target_node = parent;
                }

                if target_node.kind() == "name" || target_node.kind() == "variable" {
                    return Some(
                        self.rope
                            .slice(
                                self.rope.byte_to_char(target_node.start_byte() + offset)
                                    ..self.rope.byte_to_char(target_node.end_byte() + offset),
                            )
                            .to_string(),
                    );
                }
            }
        }
        None
    }

    pub fn find_containing_operation_node(&self, position: Position) -> Option<(tree_sitter::Node<'_>, usize)> {
        let byte_offset = self.position_to_byte(position);

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();
            let tree_len = root.end_byte();

            if byte_offset >= offset && byte_offset < offset + tree_len {
                let local_byte = byte_offset - offset;
                let trigger_node = root.descendant_for_byte_range(local_byte, local_byte)?;

                // Find containing operation
                let mut curr = trigger_node;
                while curr.kind() != "operation_definition" {
                    if let Some(parent) = curr.parent() {
                        curr = parent;
                    } else {
                        break;
                    }
                }

                if curr.kind() == "operation_definition" {
                    return Some((curr, offset));
                }
            }
        }
        None
    }

    pub fn find_variable_definition(&self, symbol_name: &str, position: Position) -> Option<Location> {
        if !symbol_name.starts_with('$') {
            return None;
        }

        if let Some((op_node, offset)) = self.find_containing_operation_node(position) {
            // Search within this operation ONLY
            let query = GQL_DEFINITION_QUERY_CACHE.get_or_init(|| {
                let lang = tree_sitter_graphql::LANGUAGE.into();
                tree_sitter::Query::new(&lang, GQL_DEFINITION_QUERY).unwrap()
            });

            let mut cursor = QueryCursor::new();
            let mut matches = cursor.matches(query, op_node, |node: tree_sitter::Node| {
                let start = node.start_byte();
                let end = node.end_byte();
                self.rope
                    .byte_slice((start + offset)..(end + offset))
                    .chunks()
            });

            while let Some(m) = matches.next() {
                let name_node = m.captures[0].node;
                let name = self
                    .rope
                    .slice(
                        self.rope.byte_to_char(name_node.start_byte() + offset)
                            ..self.rope.byte_to_char(name_node.end_byte() + offset),
                    )
                    .to_string();

                if name == symbol_name {
                    let range = self.translate_to_file_range(name_node, offset);
                    return Some(Location {
                        uri: self.uri.clone(),
                        range,
                    });
                }
            }
        }

        None
    }

    pub fn find_definition_in_tree(&self, target_name: &str) -> Option<Location> {
        if target_name.starts_with('$') {
            return None;
        }

        let query = GQL_DEFINITION_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_DEFINITION_QUERY).unwrap()
        });

        let mut cursor = QueryCursor::new();

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
                let name_node = m.captures[0].node;
                let name = self
                    .rope
                    .slice(
                        self.rope.byte_to_char(name_node.start_byte() + offset)
                            ..self.rope.byte_to_char(name_node.end_byte() + offset),
                    )
                    .to_string();

                if name == target_name {
                    let range = self.translate_to_file_range(name_node, offset);
                    return Some(Location {
                        uri: self.uri.clone(),
                        range,
                    });
                }
            }
        }

        None
    }
}
