use super::ValidationContext;
use crate::document::DocumentState;
use apollo_compiler::schema::ExtendedType;
use tower_lsp::lsp_types::*;
use tree_sitter::Node;

impl DocumentState {
    pub(super) fn validate_selection_set(
        &self,
        selection_set: Node,
        offset: usize,
        parent_type: &ExtendedType,
        ctx: &mut ValidationContext,
        depth: usize,
    ) {
        if depth > 100 {
            return;
        }
        let mut cursor = selection_set.walk();
        for child in selection_set.children(&mut cursor) {
            let kind = child.kind();

            if kind == "selection" {
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    let k = inner.kind();
                    if k == "field" {
                        self.validate_field(inner, offset, parent_type, ctx, depth + 1);
                    } else if k == "inline_fragment" {
                        self.validate_inline_fragment(inner, offset, parent_type, ctx, depth + 1);
                    } else if k == "fragment_spread" {
                        self.validate_fragment_spread(inner, offset, ctx);
                    }
                }
            } else if kind == "field" {
                self.validate_field(child, offset, parent_type, ctx, depth + 1);
            } else if kind == "fragment_spread" {
                self.validate_fragment_spread(child, offset, ctx);
            } else if kind == "inline_fragment" {
                self.validate_inline_fragment(child, offset, parent_type, ctx, depth + 1);
            }
        }
    }

    pub(super) fn validate_field(
        &self,
        field_node: Node,
        offset: usize,
        parent_type: &ExtendedType,
        ctx: &mut ValidationContext,
        depth: usize,
    ) {
        if depth > 100 {
            return;
        }
        let mut name_node = None;
        let mut selection_set_node = None;
        let mut arguments_node = None;
        let mut directives_node = None;

        let mut cursor = field_node.walk();
        for child in field_node.children(&mut cursor) {
            if child.kind() == "name" {
                name_node = Some(child);
            } else if child.kind() == "selection_set" {
                selection_set_node = Some(child);
            } else if child.kind() == "arguments" {
                arguments_node = Some(child);
            } else if child.kind() == "directives" || child.kind() == "directive" {
                directives_node = Some(child);
            }
        }

        if let Some(name_node) = name_node {
            let field_name = self.get_node_text(name_node, offset);

            // Track selected field name if we're in an operation
            if ctx.is_operation && depth == 1 {
                ctx.selected_fields.insert(field_name.clone());
            }

            if field_name == "__typename" {
                return;
            }

            let field_def = match parent_type {
                ExtendedType::Object(obj) => obj.fields.get(field_name.as_str()),
                ExtendedType::Interface(iface) => iface.fields.get(field_name.as_str()),
                _ => None,
            };

            if let Some(field_def) = field_def {
                if let Some(directive) = field_def.directives.get("deprecated") {
                    let reason = directive
                        .argument_by_name("reason", ctx.schema)
                        .ok()
                        .and_then(|arg| arg.as_str())
                        .unwrap_or("No reason provided");

                    self.add_deprecation_diagnostic(
                        ctx,
                        name_node,
                        offset,
                        format!("Field '{}' is deprecated: {}", field_name, reason),
                        reason,
                    );
                }

                if let Some(args_node) = arguments_node {
                    self.validate_arguments(args_node, offset, &field_def.arguments, ctx);
                }

                if let Some(dirs_node) = directives_node {
                    self.validate_directives(dirs_node, offset, ctx);
                }

                if let Some(sel_set) = selection_set_node {
                    let field_type_name = field_def.ty.inner_named_type();
                    if let Some(field_type_def) = ctx.schema.types.get(field_type_name.as_str()) {
                        self.validate_selection_set(
                            sel_set,
                            offset,
                            field_type_def,
                            ctx,
                            depth + 1,
                        );
                    }
                }
            } else {
                let type_name = parent_type.name();

                // Find similar field names to suggest
                let available_fields: Vec<&str> = match parent_type {
                    ExtendedType::Object(obj) => obj.fields.keys().map(|s| s.as_str()).collect(),
                    ExtendedType::Interface(iface) => {
                        iface.fields.keys().map(|s| s.as_str()).collect()
                    }
                    _ => vec![],
                };

                let similar_fields = find_similar_fields(&field_name, &available_fields);

                let message = if similar_fields.is_empty() {
                    format!("Field '{}' not found on type '{}'", field_name, type_name)
                } else {
                    format!(
                        "Field '{}' not found on type '{}'. Did you mean {}?",
                        field_name,
                        type_name,
                        similar_fields
                            .iter()
                            .map(|f| format!("'{}'", f))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };

                ctx.diagnostics.push(Diagnostic {
                    range: self.translate_to_file_range(name_node, offset),
                    severity: Some(DiagnosticSeverity::ERROR),
                    message,
                    code: Some(tower_lsp::lsp_types::NumberOrString::String(
                        "missing_field".to_string(),
                    )),
                    data: Some(serde_json::json!({
                        "similar_fields": similar_fields,
                    })),
                    ..Default::default()
                });
            }
        }
    }
}

/// Find fields with similar names using Jaro-Winkler distance
/// Returns up to 3 suggestions with similarity > 0.6
fn find_similar_fields<'a>(field_name: &str, available_fields: &[&'a str]) -> Vec<&'a str> {
    let mut similarities: Vec<(&str, f64)> = available_fields
        .iter()
        .map(|&f| (f, strsim::jaro_winkler(field_name, f)))
        .filter(|(_, score)| *score > 0.6)
        .collect();

    // Sort by similarity score (descending)
    similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Return up to 3 suggestions
    similarities.iter().take(3).map(|(name, _)| *name).collect()
}
