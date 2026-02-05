use crate::document::DocumentState;
use tower_lsp::lsp_types::*;

impl DocumentState {
    pub fn get_extraction_actions(
        &self,
        range: Range,
        schema: &apollo_compiler::Schema,
    ) -> Vec<CodeAction> {
        let mut actions = Vec::new();
        let start_byte = self.position_to_byte(range.start);
        let end_byte = self.position_to_byte(range.end);

        if start_byte == end_byte {
            return actions;
        }

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            if start_byte >= offset && end_byte <= offset + block.tree.root_node().end_byte() {
                let local_start = start_byte - offset;
                let local_end = end_byte - offset;

                let root = block.tree.root_node();
                if let Some(mut node) = root.descendant_for_byte_range(local_start, local_end) {
                    // Climb up to find a selectable node
                    while node.kind() != "selection_set" && node.kind() != "field" {
                        if let Some(parent) = node.parent() {
                            node = parent;
                        } else {
                            break;
                        }
                    }

                    // Check if node is a selection set or something we can extract
                    if node.kind() == "selection_set" || node.kind() == "field" {
                        let text = self.get_node_text(node, offset);
                        let parent_type = self.find_parent_type_for_node(node, offset, schema);
                        let type_name = parent_type
                            .as_ref()
                            .map(|t| t.name().as_str())
                            .unwrap_or("TYPE_HERE");

                        let mut changes = std::collections::HashMap::new();
                        let new_fragment_name = "NewFragment";

                        let fragment_def = format!(
                            "\n\nfragment {} on {} {{\n  {}\n}}\n",
                            new_fragment_name, type_name, text
                        );

                        // 1. Replace selection with fragment spread
                        let replace_edit = TextEdit {
                            range: self.translate_to_file_range(node, offset),
                            new_text: format!("...{}", new_fragment_name),
                        };

                        // 2. Add fragment definition at the end of the file
                        let last_line = self.rope.len_lines();
                        let append_edit = TextEdit {
                            range: Range::new(
                                Position::new(last_line as u32, 0),
                                Position::new(last_line as u32, 0),
                            ),
                            new_text: fragment_def,
                        };

                        changes.insert(self.uri.clone(), vec![replace_edit, append_edit]);

                        actions.push(CodeAction {
                            title: "Extract to fragment".to_string(),
                            kind: Some(CodeActionKind::REFACTOR_EXTRACT),
                            edit: Some(WorkspaceEdit {
                                changes: Some(changes),
                                ..Default::default()
                            }),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        actions
    }

    pub fn get_unused_fragment_actions(&self, diagnostic: &Diagnostic) -> Vec<CodeAction> {
        let mut actions = Vec::new();
        let range = diagnostic.range;
        let start_byte = self.position_to_byte(range.start);
        let end_byte = self.position_to_byte(range.end);

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            if start_byte >= offset && end_byte <= offset + block.tree.root_node().end_byte() {
                let local_start = start_byte - offset;
                let local_end = end_byte - offset;
                let root = block.tree.root_node();

                if let Some(mut node) = root.descendant_for_byte_range(local_start, local_end) {
                    // Climb up to find the fragment definition
                    while node.kind() != "fragment_definition" {
                        if let Some(parent) = node.parent() {
                            node = parent;
                        } else {
                            break;
                        }
                    }

                    if node.kind() == "fragment_definition" {
                        // Find where to insert @type_only
                        let mut insertion_point = None;
                        let mut cursor = node.walk();
                        for child in node.children(&mut cursor) {
                            if child.kind() == "type_condition" {
                                insertion_point =
                                    Some(self.byte_to_position(child.end_byte() + offset));
                            }
                        }

                        if let Some(pos) = insertion_point {
                            let mut changes = std::collections::HashMap::new();
                            changes.insert(
                                self.uri.clone(),
                                vec![TextEdit {
                                    range: Range::new(pos, pos),
                                    new_text: " @type_only".to_string(),
                                }],
                            );

                            actions.push(CodeAction {
                                title: "Mark fragment as @type_only (If this fragment is only used for Typescript type and not used in any query)".to_string(),
                                kind: Some(CodeActionKind::QUICKFIX),
                                diagnostics: Some(vec![diagnostic.clone()]),
                                edit: Some(WorkspaceEdit {
                                    changes: Some(changes),
                                    ..Default::default()
                                }),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
        }
        actions
    }
}
