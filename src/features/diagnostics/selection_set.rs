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
        let mut alias_node = None;
        let mut selection_set_node = None;
        let mut arguments_node = None;
        let mut directives_node = None;

        let mut cursor = field_node.walk();
        for child in field_node.children(&mut cursor) {
            if child.kind() == "alias" {
                alias_node = Some(child);
            } else if child.kind() == "name" {
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
            // Determine response key: prefer alias (if present) otherwise field name
            let field_name = if let Some(alias_node) = alias_node {
                // alias_node likely contains a name child; find it
                let mut a_cursor = alias_node.walk();
                let mut found = None;
                for a_child in alias_node.children(&mut a_cursor) {
                    if a_child.kind() == "name" {
                        found = Some(self.get_node_text(a_child, offset));
                        break;
                    }
                }
                found.unwrap_or_else(|| self.get_node_text(name_node, offset))
            } else {
                self.get_node_text(name_node, offset)
            };

            // Check for duplicate field names in the same selection set (shallow check)
            // This rule is configurable via config.rules.no_duplicate_fields
            if let Some(cfg) = ctx.config
                && let Some(rules) = &cfg.rules
                && let Some(true) = rules.no_duplicate_fields
            {
                // Only consider shallow duplicates within the same selection set level
                // We use depth to ensure we only check within the current selection set
                if depth >= 1 {
                    // Track seen names locally per selection set via a temporary set on the context
                    // We don't want this to span fragments, so this check is only per invocation
                    // of validate_selection_set; implement by using a synthetic key combining
                    // the parent node's start_byte and current depth.
                    // For simplicity, we do a local scan of sibling fields here.
                    if let Some(parent) = field_node.parent() {
                        // Scan siblings for earlier occurrences of the same response key
                        // We consider two fields to be equivalent (and therefore NOT duplicates)
                        // if they have the same response key (alias or name), identical
                        // arguments text (or both absent), and identical selection sets (or both absent).
                        // If a prior sibling has the same response key but different arguments or selection
                        // set, we treat that as a duplicate conflict.
                        let mut seen_conflict = false;
                        let mut cursor = parent.walk();

                        // Get current field's arguments and selection set text (if any)
                        let field_args_text = arguments_node
                            .map(|n| self.get_node_text(n, offset))
                            .unwrap_or_default();
                        let field_sel_text = selection_set_node
                            .map(|n| self.get_node_text(n, offset))
                            .unwrap_or_default();

                        for sibling in parent.children(&mut cursor) {
                            if sibling.id() == field_node.id() {
                                // stop scanning once we reach ourselves
                                break;
                            }

                            if sibling.kind() == "selection" || sibling.kind() == "field" {
                                // find sibling response key (alias or name), args, and selection text
                                let mut s_cursor = sibling.walk();
                                let mut sibling_key = None;
                                let mut sibling_args = String::new();
                                let mut sibling_sel = String::new();

                                for s_child in sibling.children(&mut s_cursor) {
                                    if s_child.kind() == "alias" {
                                        // alias node contains a name child
                                        let mut a_cursor = s_child.walk();
                                        for a_child in s_child.children(&mut a_cursor) {
                                            if a_child.kind() == "name" {
                                                sibling_key =
                                                    Some(self.get_node_text(a_child, offset));
                                                break;
                                            }
                                        }
                                    } else if s_child.kind() == "name" {
                                        if sibling_key.is_none() {
                                            sibling_key = Some(self.get_node_text(s_child, offset));
                                        }
                                    } else if s_child.kind() == "arguments" {
                                        sibling_args = self.get_node_text(s_child, offset);
                                    } else if s_child.kind() == "selection_set" {
                                        sibling_sel = self.get_node_text(s_child, offset);
                                    }
                                }

                                if let Some(s_key) = sibling_key
                                    && s_key == field_name
                                {
                                    // same response key; check if args and selection match
                                    if sibling_args != field_args_text
                                        || sibling_sel != field_sel_text
                                    {
                                        seen_conflict = true;
                                        break;
                                    } else {
                                        // identical field occurrence (same args/selection): ignore
                                        // continue scanning
                                    }
                                }
                            }
                        }

                        if seen_conflict {
                            ctx.diagnostics.push(Diagnostic {
                                range: self.translate_to_file_range(name_node, offset),
                                severity: Some(DiagnosticSeverity::ERROR),
                                message: format!(
                                    "Duplicate field '{}' in selection set",
                                    field_name
                                ),
                                code: Some(tower_lsp::lsp_types::NumberOrString::String(
                                    "no_duplicate_fields".to_string(),
                                )),
                                // Attach contextual data so code action can compute a better removal
                                data: Some(serde_json::json!({
                                    "response_key": field_name,
                                    "args": field_args_text,
                                    "selection": field_sel_text,
                                })),
                                ..Default::default()
                            });
                        }
                    }
                }
            }

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
