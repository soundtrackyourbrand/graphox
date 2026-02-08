use graphql_core::document::DocumentState;
use lsp_types::*;

pub trait DocumentCodeActions {
    fn get_format_action(&self, range: Range) -> Option<CodeAction>;
    fn get_extraction_actions(
        &self,
        range: Range,
        schema: &apollo_compiler::Schema,
    ) -> Vec<CodeAction>;
    fn get_unused_fragment_actions(&self, diagnostic: &Diagnostic) -> Vec<CodeAction>;
    fn get_missing_field_actions(&self, diagnostic: &Diagnostic) -> Vec<CodeAction>;
    fn get_duplicate_field_actions(&self, diagnostic: &Diagnostic) -> Vec<CodeAction>;
    fn get_required_field_actions(&self, diagnostic: &Diagnostic) -> Vec<CodeAction>;
}

impl DocumentCodeActions for DocumentState {
    /// Get format action for inline GraphQL blocks in TypeScript/JavaScript files
    fn get_format_action(&self, range: Range) -> Option<CodeAction> {
        // Only provide formatting for host language files (TS/TSX)
        if !self.language.is_host_language() {
            return None;
        }

        let start_byte = self.position_to_byte(range.start);

        // Find which GraphQL block contains the cursor/range
        for block in self.get_graphql_trees() {
            let block_start = block.offset;
            let block_end = block.offset + block.tree.root_node().end_byte();

            // Check if the position is within this GraphQL block
            if start_byte >= block_start && start_byte <= block_end {
                // Extract the GraphQL content
                let graphql_content = self.rope.byte_slice(block_start..block_end).to_string();

                // Parse and format using apollo-compiler
                // We use the AST parser which doesn't require a schema
                let mut parser = apollo_compiler::parser::Parser::new();
                let doc = parser.parse_ast(graphql_content.clone(), "inline.graphql");

                // Handle parse result - even with errors, we can get a partial document
                let formatted = match doc {
                    Ok(document)
                    | Err(apollo_compiler::validation::WithErrors {
                        partial: document, ..
                    }) => {
                        // apollo-compiler's Document implements Display which formats the GraphQL
                        document.to_string()
                    }
                };

                // Only create the action if the formatted version is different
                if formatted.trim() == graphql_content.trim() {
                    return None;
                }

                let mut changes = std::collections::HashMap::new();
                changes.insert(
                    self.uri.clone(),
                    vec![TextEdit {
                        range: Range {
                            start: self.byte_to_position(block_start),
                            end: self.byte_to_position(block_end),
                        },
                        new_text: formatted,
                    }],
                );

                return Some(CodeAction {
                    title: "Format GraphQL".to_string(),
                    kind: Some(CodeActionKind::SOURCE_FIX_ALL),
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        ..Default::default()
                    }),
                    ..Default::default()
                });
            }
        }

        None
    }

    fn get_extraction_actions(
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

    fn get_unused_fragment_actions(&self, diagnostic: &Diagnostic) -> Vec<CodeAction> {
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

    /// Get code actions for missing field diagnostics - suggests replacing with similar field names
    fn get_missing_field_actions(&self, diagnostic: &Diagnostic) -> Vec<CodeAction> {
        let mut actions = Vec::new();

        // Extract similar fields from diagnostic data
        let similar_fields: Vec<String> = if let Some(data) = &diagnostic.data {
            if let Some(fields) = data.get("similar_fields") {
                serde_json::from_value(fields.clone()).unwrap_or_default()
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        // Create a code action for each similar field
        for similar_field in similar_fields {
            let mut changes = std::collections::HashMap::new();
            changes.insert(
                self.uri.clone(),
                vec![TextEdit {
                    range: diagnostic.range,
                    new_text: similar_field.clone(),
                }],
            );

            actions.push(CodeAction {
                title: format!("Change to '{}'", similar_field),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diagnostic.clone()]),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }),
                is_preferred: None,
                ..Default::default()
            });
        }

        actions
    }

    /// Get code actions for duplicate field diagnostics - remove the duplicated field
    fn get_duplicate_field_actions(&self, diagnostic: &Diagnostic) -> Vec<CodeAction> {
        let mut actions = Vec::new();
        let range = diagnostic.range;

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let start_byte = self.position_to_byte(range.start);
            let end_byte = self.position_to_byte(range.end);
            if start_byte >= offset && end_byte <= offset + block.tree.root_node().end_byte() {
                // Find the field name node and then the containing selection/field node to remove
                let local_start = start_byte - offset;
                let local_end = end_byte - offset;
                let root = block.tree.root_node();
                if let Some(mut node) = root.descendant_for_byte_range(local_start, local_end) {
                    // climb to the field node
                    while node.kind() != "field" && node.kind() != "selection" {
                        if let Some(parent) = node.parent() {
                            node = parent;
                        } else {
                            break;
                        }
                    }

                    if node.kind() == "field" || node.kind() == "selection" {
                        // Compute a removal range that preserves formatting:
                        // - If the field starts at the beginning (after indentation) of its line and
                        //   the text before it on the line is only whitespace, remove that entire line
                        //   including the trailing newline so no empty line remains.
                        // - Otherwise, remove exactly the node range.
                        let abs_start = offset + node.start_byte();
                        let abs_end = offset + node.end_byte();

                        // Determine start of the line containing the node
                        let start_pos = self.byte_to_position(abs_start);
                        let line_start_byte =
                            self.position_to_byte(Position::new(start_pos.line, 0));

                        let mut remove_start = abs_start;
                        // If all bytes between line_start_byte and abs_start are whitespace, expand to line start
                        if line_start_byte < abs_start {
                            let before_text =
                                self.rope.byte_slice(line_start_byte..abs_start).to_string();
                            if before_text.trim().is_empty() {
                                remove_start = line_start_byte;
                            }
                        }

                        // Determine end of next line start to include trailing newline
                        let total_lines = self.rope.len_lines() as u32;
                        let end_pos = self.byte_to_position(abs_end);
                        let mut remove_end = abs_end;
                        if end_pos.line + 1 < total_lines {
                            let next_line_start =
                                self.position_to_byte(Position::new(end_pos.line + 1, 0));
                            // If the remainder of the line after the node is only whitespace, include the newline
                            let after_text =
                                self.rope.byte_slice(abs_end..next_line_start).to_string();
                            if after_text.trim().is_empty() {
                                remove_end = next_line_start;
                            }
                        }

                        let start_pos = self.byte_to_position(remove_start);
                        let end_pos = self.byte_to_position(remove_end);

                        let text_range = Range {
                            start: start_pos,
                            end: end_pos,
                        };
                        let mut changes = std::collections::HashMap::new();
                        changes.insert(
                            self.uri.clone(),
                            vec![TextEdit {
                                range: text_range,
                                new_text: String::new(),
                            }],
                        );

                        // Build code action and copy diagnostic data if present so clients can
                        // make informed edits or present richer UI.
                        let mut ca = CodeAction {
                            title: "Remove duplicate field".to_string(),
                            kind: Some(CodeActionKind::QUICKFIX),
                            diagnostics: Some(vec![diagnostic.clone()]),
                            edit: Some(WorkspaceEdit {
                                changes: Some(changes),
                                ..Default::default()
                            }),
                            is_preferred: Some(true),
                            ..Default::default()
                        };

                        if let Some(data) = &diagnostic.data {
                            ca.data = Some(data.clone());
                        }

                        actions.push(ca);
                    }
                }
            }
        }
        actions
    }

    /// Get code actions for required field diagnostics - adds the missing required field
    fn get_required_field_actions(&self, diagnostic: &Diagnostic) -> Vec<CodeAction> {
        let mut actions = Vec::new();

        // Extract the field name from the diagnostic message
        // Message format: "Required field 'fieldName' must be selected in <type> operations"
        let field_name = if let Some(start) = diagnostic.message.find('\'') {
            if let Some(end) = diagnostic.message[start + 1..].find('\'') {
                &diagnostic.message[start + 1..start + 1 + end]
            } else {
                return actions;
            }
        } else {
            return actions;
        };

        // Find the operation in the diagnostic range
        let start_byte = self.position_to_byte(diagnostic.range.start);
        let end_byte = self.position_to_byte(diagnostic.range.end);

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            if start_byte >= offset && end_byte <= offset + block.tree.root_node().end_byte() {
                let local_start = start_byte - offset;
                let local_end = end_byte - offset;
                let root = block.tree.root_node();

                if let Some(mut node) = root.descendant_for_byte_range(local_start, local_end) {
                    // Climb up to find the operation definition
                    while node.kind() != "operation_definition" {
                        if let Some(parent) = node.parent() {
                            node = parent;
                        } else {
                            break;
                        }
                    }

                    if node.kind() == "operation_definition" {
                        // Find the selection set
                        let mut cursor = node.walk();
                        for child in node.children(&mut cursor) {
                            if child.kind() == "selection_set" {
                                // Find the position right after the opening brace
                                let insert_position =
                                    self.byte_to_position(child.start_byte() + offset + 1);

                                // Get the indentation of the first field (if any)
                                let mut indentation = "\n  ".to_string();
                                let mut has_fields = false;

                                let mut selection_cursor = child.walk();
                                for selection_child in child.children(&mut selection_cursor) {
                                    if selection_child.kind() == "selection" {
                                        has_fields = true;
                                        // Extract indentation from first field
                                        let field_start_pos = self.byte_to_position(
                                            selection_child.start_byte() + offset,
                                        );
                                        let field_start_byte = self.position_to_byte(
                                            Position::new(field_start_pos.line, 0),
                                        );
                                        let field_actual_start =
                                            selection_child.start_byte() + offset;

                                        if field_actual_start > field_start_byte {
                                            let indent_text = self
                                                .rope
                                                .byte_slice(field_start_byte..field_actual_start)
                                                .to_string();
                                            indentation = format!("\n{}", indent_text);
                                        }
                                        break;
                                    }
                                }

                                let new_text = if has_fields {
                                    format!("{}{}", indentation, field_name)
                                } else {
                                    // Empty selection set, add with default indentation
                                    format!("\n  {}\n", field_name)
                                };

                                let mut changes = std::collections::HashMap::new();
                                changes.insert(
                                    self.uri.clone(),
                                    vec![TextEdit {
                                        range: Range::new(insert_position, insert_position),
                                        new_text,
                                    }],
                                );

                                actions.push(CodeAction {
                                    title: format!("Add required field '{}'", field_name),
                                    kind: Some(CodeActionKind::QUICKFIX),
                                    diagnostics: Some(vec![diagnostic.clone()]),
                                    edit: Some(WorkspaceEdit {
                                        changes: Some(changes),
                                        ..Default::default()
                                    }),
                                    is_preferred: Some(true),
                                    ..Default::default()
                                });

                                break;
                            }
                        }
                    }
                }
            }
        }

        actions
    }
}
