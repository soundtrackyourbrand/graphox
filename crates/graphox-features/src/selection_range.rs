use graphox_core::document::DocumentState;
use lsp_types::*;

pub trait DocumentSelectionRange {
    fn get_selection_ranges(&self, positions: Vec<Position>) -> Vec<SelectionRange>;
    fn get_selection_range_at_position(&self, position: Position) -> Option<SelectionRange>;
    fn position_to_byte_offset(&self, position: Position, block_offset: usize) -> Option<usize>;
}

impl DocumentSelectionRange for DocumentState {
    /// Returns selection ranges for the given positions
    /// Selection ranges allow editors to expand/contract selections based on syntax tree
    fn get_selection_ranges(&self, positions: Vec<Position>) -> Vec<SelectionRange> {
        positions
            .into_iter()
            .filter_map(|pos| self.get_selection_range_at_position(pos))
            .collect()
    }

    /// Gets the selection range at a specific position
    fn get_selection_range_at_position(&self, position: Position) -> Option<SelectionRange> {
        // Try to find the node at this position in any GraphQL block
        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let byte_offset = self.position_to_byte_offset(position, offset)?;
            let root = block.tree.root_node();

            // Find the deepest node at this position
            let mut node = root.descendant_for_byte_range(byte_offset, byte_offset)?;

            // Build the selection range chain from innermost to outermost
            let mut current_range: Option<SelectionRange> = None;

            loop {
                let node_range = self.translate_to_file_range(node, offset);

                // Skip nodes that are zero-width or invalid
                if node_range.start == node_range.end {
                    if let Some(parent) = node.parent() {
                        node = parent;
                        continue;
                    } else {
                        break;
                    }
                }

                // Create a selection range for this node
                let selection_range = SelectionRange {
                    range: node_range,
                    parent: current_range.map(Box::new),
                };

                current_range = Some(selection_range);

                // Move to parent node
                if let Some(parent) = node.parent() {
                    node = parent;
                } else {
                    break;
                }
            }

            // Return the innermost selection range (which has all parents linked)
            if current_range.is_some() {
                return current_range;
            }
        }

        None
    }

    /// Helper to convert LSP Position to byte offset in the document
    fn position_to_byte_offset(&self, position: Position, block_offset: usize) -> Option<usize> {
        let line_idx = position.line as usize;
        if line_idx >= self.rope.len_lines() {
            return None;
        }

        let line_start = self.rope.line_to_byte(line_idx);
        let line = self.rope.line(line_idx);

        // Calculate character offset in bytes (handling UTF-8)
        let mut char_count = 0;
        let mut byte_offset = 0;

        for chunk in line.chunks() {
            for ch in chunk.chars() {
                if char_count >= position.character as usize {
                    return Some(line_start + byte_offset - block_offset);
                }
                char_count += 1;
                byte_offset += ch.len_utf8();
            }
        }

        // If we've reached the end of the line
        if char_count == position.character as usize {
            Some(line_start + byte_offset - block_offset)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_doc(uri_str: &str, text: &str) -> DocumentState {
        let uri = Url::parse(uri_str).unwrap();
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_graphql::LANGUAGE.into())
            .unwrap();
        DocumentState::new(uri, text, parser)
    }

    #[test]
    fn test_selection_range_field() {
        let text = r#"query GetUser {
  user {
    id
    name
  }
}"#;
        let doc = create_doc("file:///test.graphql", text);

        // Position on "name" field
        let position = Position {
            line: 3,
            character: 6,
        };

        let ranges = doc.get_selection_ranges(vec![position]);
        assert_eq!(ranges.len(), 1);

        // Should have a chain of parent ranges
        let mut current = &ranges[0];
        let mut depth = 0;
        while current.parent.is_some() {
            depth += 1;
            current = current.parent.as_ref().unwrap();
        }

        // Should have multiple levels: field -> selection_set -> operation, etc.
        assert!(depth >= 3, "Should have multiple selection range levels");
    }

    #[test]
    fn test_selection_range_multiple_positions() {
        let text = r#"query GetUser {
  user {
    id
    name
  }
}"#;
        let doc = create_doc("file:///test.graphql", text);

        let positions = vec![
            Position {
                line: 2,
                character: 4,
            }, // "id"
            Position {
                line: 3,
                character: 4,
            }, // "name"
        ];

        let ranges = doc.get_selection_ranges(positions);
        assert_eq!(ranges.len(), 2, "Should return ranges for all positions");
    }

    #[test]
    fn test_selection_range_fragment() {
        let text = r#"fragment UserFields on User {
  id
  name
  email
}"#;
        let doc = create_doc("file:///test.graphql", text);

        let position = Position {
            line: 2,
            character: 3,
        };

        let ranges = doc.get_selection_ranges(vec![position]);
        assert_eq!(ranges.len(), 1);

        // Verify we get a valid range
        assert!(ranges[0].parent.is_some(), "Should have parent ranges");
    }
}
