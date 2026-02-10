use crate::references::DocumentReferences;
use crate::shared::type_resolver::SemanticSymbol;
use crate::shared::type_resolver::resolve_symbol_at_node;
use apollo_compiler::Schema;
use graphox_core::document::DocumentState;
use lsp_types::*;

pub trait DocumentHighlightFeature {
    fn get_document_highlights(
        &self,
        position: Position,
        schema: &Schema,
    ) -> Option<Vec<DocumentHighlight>>;
    fn is_variable_definition_at_range(
        &self,
        range: Range,
        op_node: tree_sitter::Node,
        offset: usize,
    ) -> bool;
    fn get_variable_highlights(
        &self,
        position: Position,
        symbol_name: &str,
    ) -> Option<Vec<DocumentHighlight>>;
    fn get_fragment_or_operation_highlights(
        &self,
        position: Position,
        schema: &Schema,
    ) -> Option<Vec<DocumentHighlight>>;
}

impl DocumentHighlightFeature for DocumentState {
    fn get_document_highlights(
        &self,
        position: Position,
        schema: &Schema,
    ) -> Option<Vec<DocumentHighlight>> {
        let symbol_name = self.get_symbol_at_position(position)?;

        if symbol_name.starts_with('$') {
            self.get_variable_highlights(position, &symbol_name)
        } else {
            self.get_fragment_or_operation_highlights(position, schema)
        }
    }

    fn is_variable_definition_at_range(
        &self,
        range: Range,
        op_node: tree_sitter::Node,
        offset: usize,
    ) -> bool {
        let start_byte = self.position_to_byte(range.start);
        let local_byte = start_byte - offset;

        if let Some(node) = op_node.descendant_for_byte_range(local_byte, local_byte) {
            let mut curr = node;
            loop {
                if curr.kind() == "variable_definition" {
                    return true;
                }
                if curr == op_node {
                    break;
                }
                if let Some(parent) = curr.parent() {
                    curr = parent;
                } else {
                    break;
                }
            }
        }

        false
    }

    fn get_variable_highlights(
        &self,
        position: Position,
        symbol_name: &str,
    ) -> Option<Vec<DocumentHighlight>> {
        let (op_node, offset) = self.find_containing_operation_node(position)?;

        let mut locations = self.find_variable_references(symbol_name, position, true);

        let fragment_spreads = self.get_fragment_spreads_in_node(op_node, offset);
        for spread_name in fragment_spreads {
            if self.fragments().iter().any(|f| f.name == spread_name) {
                let frag_refs = self.find_references_in_tree(symbol_name, false);
                locations.extend(frag_refs);
            }
        }

        let highlights: Vec<DocumentHighlight> = locations
            .into_iter()
            .map(|loc| {
                let kind = if self.is_variable_definition_at_range(loc.range, op_node, offset) {
                    DocumentHighlightKind::WRITE
                } else {
                    DocumentHighlightKind::READ
                };

                DocumentHighlight {
                    range: loc.range,
                    kind: Some(kind),
                }
            })
            .collect();

        if highlights.is_empty() {
            None
        } else {
            Some(highlights)
        }
    }

    fn get_fragment_or_operation_highlights(
        &self,
        position: Position,
        schema: &Schema,
    ) -> Option<Vec<DocumentHighlight>> {
        let cursor_offset = self.position_to_byte(position);

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();
            let tree_len = root.end_byte();

            if cursor_offset >= offset && cursor_offset < offset + tree_len {
                let local_byte = cursor_offset - offset;
                if let Some(node) = root.descendant_for_byte_range(local_byte, local_byte)
                    && let Some(symbol) =
                        resolve_symbol_at_node(self, node, offset, cursor_offset, schema)
                {
                    match symbol {
                        SemanticSymbol::Fragment { name, .. } => {
                            let locations = self.find_references_in_tree(&name, true);
                            return Some(
                                locations
                                    .into_iter()
                                    .map(|loc| DocumentHighlight {
                                        range: loc.range,
                                        kind: Some(DocumentHighlightKind::READ),
                                    })
                                    .collect(),
                            );
                        }
                        SemanticSymbol::Operation { name, .. } => {
                            if let Some(op_name) = name {
                                let locations = self.find_references_in_tree(&op_name, true);
                                return Some(
                                    locations
                                        .into_iter()
                                        .map(|loc| DocumentHighlight {
                                            range: loc.range,
                                            kind: Some(DocumentHighlightKind::READ),
                                        })
                                        .collect(),
                                );
                            }
                            return None;
                        }
                        _ => return None,
                    }
                }
            }
        }

        None
    }
}
