use crate::definition::DocumentDefinition;
use crate::references::DocumentReferences;
use graphql_core::document::DocumentState;
use lsp_types::*;

pub trait DocumentHighlightFeature {
    fn get_document_highlights(&self, position: Position) -> Option<Vec<DocumentHighlight>>;
    fn is_variable_definition_at_range(
        &self,
        range: Range,
        op_node: tree_sitter::Node,
        offset: usize,
    ) -> bool;
}

impl DocumentHighlightFeature for DocumentState {
    /// Get document highlights for a variable at the given position.
    /// This returns all occurrences of the variable within the same document,
    /// including in fragments that are spread into the operation.
    fn get_document_highlights(&self, position: Position) -> Option<Vec<DocumentHighlight>> {
        // Get symbol at position
        let symbol_name = self.get_symbol_at_position(position)?;

        // Only handle variables (starts with $)
        if !symbol_name.starts_with('$') {
            return None;
        }

        // Find all variable references in the containing operation
        let (op_node, offset) = self.find_containing_operation_node(position)?;

        // Get variable references and definitions in the operation
        let mut locations = self.find_variable_references(&symbol_name, position, true);

        // Also find references in fragments defined in this same file that are used by the operation
        let fragment_spreads = self.get_fragment_spreads_in_node(op_node, offset);
        for spread_name in fragment_spreads {
            // Check if this fragment is defined in the current document
            if self.fragments().iter().any(|f| f.name == spread_name) {
                // Find references to the variable in this fragment
                let frag_refs = self.find_references_in_tree(&symbol_name, false);
                locations.extend(frag_refs);
            }
        }

        // Convert locations to document highlights
        let highlights: Vec<DocumentHighlight> = locations
            .into_iter()
            .map(|loc| {
                // Determine if this is a write (definition) or read (reference)
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

    fn is_variable_definition_at_range(
        &self,
        range: Range,
        op_node: tree_sitter::Node,
        offset: usize,
    ) -> bool {
        // Convert range to byte offset
        let start_byte = self.position_to_byte(range.start);
        let local_byte = start_byte - offset;

        // Get the node at this position
        if let Some(node) = op_node.descendant_for_byte_range(local_byte, local_byte) {
            // Walk up the tree to check if we're in a variable_definition
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
}
