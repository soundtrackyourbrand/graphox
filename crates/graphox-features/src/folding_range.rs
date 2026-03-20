use graphox_core::document::DocumentState;
use lsp_types::*;
use tree_sitter::Node;

pub trait DocumentFoldingRange {
    fn get_folding_ranges(&self) -> Vec<FoldingRange>;
    fn collect_folding_ranges(&self, node: Node, offset: usize, ranges: &mut Vec<FoldingRange>);
    fn node_to_folding_range(&self, node: Node, offset: usize) -> Option<FoldingRange>;
}

impl DocumentFoldingRange for DocumentState {
    /// Returns folding ranges for the document
    fn get_folding_ranges(&self) -> Vec<FoldingRange> {
        let mut ranges = Vec::new();

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();

            // Walk the tree and collect foldable regions
            self.collect_folding_ranges(root, offset, &mut ranges);
        }

        ranges
    }

    fn collect_folding_ranges(&self, node: Node, offset: usize, ranges: &mut Vec<FoldingRange>) {
        // Check if this node is foldable
        if let Some(range) = self.node_to_folding_range(node, offset) {
            ranges.push(range);
        }

        // Skip string_value and comment nodes - their content is opaque text
        if node.kind() == "string_value" || node.kind() == "comment" {
            return;
        }

        // Recursively process children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_folding_ranges(child, offset, ranges);
        }
    }

    fn node_to_folding_range(&self, node: Node, offset: usize) -> Option<FoldingRange> {
        let kind = node.kind();

        // Determine if this node should be foldable and its kind
        let folding_kind = match kind {
            // Foldable GraphQL structures
            "selection_set" => Some(FoldingRangeKind::Region),
            "object_value" => Some(FoldingRangeKind::Region),
            "list_value" => Some(FoldingRangeKind::Region),
            "arguments" => Some(FoldingRangeKind::Region),
            "variable_definitions" => Some(FoldingRangeKind::Region),
            "directives" => Some(FoldingRangeKind::Region),

            // Definition blocks
            "operation_definition" => Some(FoldingRangeKind::Region),
            "fragment_definition" => Some(FoldingRangeKind::Region),
            "inline_fragment" => Some(FoldingRangeKind::Region),

            // Schema definitions
            "object_type_definition" => Some(FoldingRangeKind::Region),
            "interface_type_definition" => Some(FoldingRangeKind::Region),
            "enum_type_definition" => Some(FoldingRangeKind::Region),
            "union_type_definition" => Some(FoldingRangeKind::Region),
            "input_object_type_definition" => Some(FoldingRangeKind::Region),
            "scalar_type_definition" => Some(FoldingRangeKind::Region),
            "directive_definition" => Some(FoldingRangeKind::Region),

            // Schema extensions
            "object_type_extension" => Some(FoldingRangeKind::Region),
            "interface_type_extension" => Some(FoldingRangeKind::Region),
            "enum_type_extension" => Some(FoldingRangeKind::Region),
            "union_type_extension" => Some(FoldingRangeKind::Region),
            "input_object_type_extension" => Some(FoldingRangeKind::Region),
            "scalar_type_extension" => Some(FoldingRangeKind::Region),

            // Comments
            "comment" => Some(FoldingRangeKind::Comment),
            "description" => Some(FoldingRangeKind::Comment),

            _ => None,
        }?;

        // Get the range of the node in the file
        let file_range = self.translate_to_file_range(node, offset);

        // Only create folding ranges for multi-line regions
        if file_range.end.line <= file_range.start.line {
            return None;
        }

        // For certain node types, we want to fold only the body, not the header
        let (start_line, end_line) = match kind {
            "operation_definition"
            | "fragment_definition"
            | "object_type_definition"
            | "interface_type_definition"
            | "enum_type_definition"
            | "union_type_definition"
            | "input_object_type_definition"
            | "directive_definition"
            | "object_type_extension"
            | "interface_type_extension"
            | "enum_type_extension"
            | "union_type_extension"
            | "input_object_type_extension" => {
                // Find the selection_set or field_definitions child
                let mut cursor = node.walk();
                let mut body_start_line = file_range.start.line;

                for child in node.children(&mut cursor) {
                    let child_kind = child.kind();
                    if child_kind == "selection_set"
                        || child_kind == "fields_definition"
                        || child_kind == "enum_values_definition"
                        || child_kind == "input_fields_definition"
                        || child_kind == "argument_definitions"
                    {
                        let child_range = self.translate_to_file_range(child, offset);
                        body_start_line = child_range.start.line;
                        break;
                    }
                }

                (body_start_line, file_range.end.line)
            }
            _ => (file_range.start.line, file_range.end.line),
        };

        // Final check for multi-line
        if end_line <= start_line {
            return None;
        }

        Some(FoldingRange {
            start_line,
            start_character: None,
            end_line,
            end_character: None,
            kind: Some(folding_kind),
            collapsed_text: None,
        })
    }
}
