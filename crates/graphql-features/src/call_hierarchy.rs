use crate::definition::DocumentDefinition;
use graphql_core::document::DocumentState;
use graphql_core::queries::*;
use lsp_types::*;
use tree_sitter::{QueryCursor, StreamingIterator};

pub trait DocumentCallHierarchy {
    fn prepare_call_hierarchy(&self, position: Position) -> Option<Vec<CallHierarchyItem>>;
    fn get_outgoing_calls(&self, symbol_name: &str) -> Vec<CallHierarchyOutgoingCall>;
    fn get_container_name_at_range(&self, range: Range) -> Option<String>;
}

impl DocumentCallHierarchy for DocumentState {
    fn prepare_call_hierarchy(&self, position: Position) -> Option<Vec<CallHierarchyItem>> {
        let symbol_name = self.get_symbol_at_position(position)?;

        // Find the definition to get the range for the item
        let location = self.find_definition_in_tree(&symbol_name)?;

        Some(vec![CallHierarchyItem {
            name: symbol_name.clone(),
            kind: SymbolKind::FUNCTION, // fragments are like functions
            tags: None,
            detail: Some("fragment".to_string()),
            uri: location.uri,
            range: location.range,
            selection_range: location.range,
            data: Some(serde_json::to_value(symbol_name).unwrap()),
        }])
    }

    fn get_outgoing_calls(&self, symbol_name: &str) -> Vec<CallHierarchyOutgoingCall> {
        let mut calls = Vec::new();
        let query = GQL_REFERENCES_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_REFERENCES_QUERY).unwrap()
        });

        let mut cursor = QueryCursor::new();
        let reference_idx = query.capture_index_for_name("reference").unwrap();

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();

            // Find the container (fragment or operation) for this symbol
            let mut container_node = None;
            let mut matches = cursor.matches(query, root, |node: tree_sitter::Node| {
                let start = node.start_byte();
                let end = node.end_byte();
                self.rope
                    .byte_slice((start + offset)..(end + offset))
                    .chunks()
            });

            while let Some(m) = matches.next() {
                let mut is_definition = false;
                let mut name_node = None;
                let definition_idx = query.capture_index_for_name("definition").unwrap();

                for cap in m.captures {
                    if cap.index == definition_idx {
                        is_definition = true;
                    } else if query.capture_names()[cap.index as usize] == "name" {
                        name_node = Some(cap.node);
                    }
                }

                if is_definition && let Some(name_node) = name_node {
                    let name = self.get_node_text(name_node, offset);
                    if name == symbol_name {
                        container_node = Some(m.captures[0].node); // fragment_definition
                        break;
                    }
                }
            }

            if let Some(container) = container_node {
                // Now find all fragment spreads inside THIS container
                let mut inner_cursor = QueryCursor::new();
                let mut inner_matches =
                    inner_cursor.matches(query, container, |node: tree_sitter::Node| {
                        let start = node.start_byte();
                        let end = node.end_byte();
                        self.rope
                            .byte_slice((start + offset)..(end + offset))
                            .chunks()
                    });

                while let Some(m) = inner_matches.next() {
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
                        let callee_name = self.get_node_text(name_node, offset);
                        let range = self.translate_to_file_range(name_node, offset);

                        // We don't have the URI of the callee easily here,
                        // so we just return the name and let the backend resolve the item.
                        // Actually CallHierarchyOutgoingCall needs a CallHierarchyItem for the callee.
                        // This is tricky because we don't know where it's defined yet.
                        // We'll return a "partial" item and let the backend fill it.

                        calls.push(CallHierarchyOutgoingCall {
                            to: CallHierarchyItem {
                                name: callee_name,
                                kind: SymbolKind::FUNCTION,
                                tags: None,
                                detail: None,
                                uri: self.uri.clone(),   // Placeholder
                                range: Range::default(), // Placeholder
                                selection_range: Range::default(), // Placeholder
                                data: None,
                            },
                            from_ranges: vec![range],
                        });
                    }
                }
            }
        }

        calls
    }

    fn get_container_name_at_range(&self, range: Range) -> Option<String> {
        let byte_offset = self.position_to_byte(range.start);

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();

            if byte_offset >= offset && byte_offset <= offset + root.end_byte() {
                let local_byte = byte_offset - offset;
                let mut node = root.descendant_for_byte_range(local_byte, local_byte);

                while let Some(current) = node {
                    if current.kind() == "fragment_definition"
                        || current.kind() == "operation_definition"
                    {
                        let mut cursor = current.walk();
                        for child in current.children(&mut cursor) {
                            if child.kind() == "fragment_name" || child.kind() == "name" {
                                return Some(self.get_node_text(child, offset));
                            }
                        }
                    }
                    node = current.parent();
                }
            }
        }
        None
    }
}
