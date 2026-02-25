use super::DIAGNOSTIC_SOURCE;
use super::ValidationContext;
use apollo_compiler::schema::ExtendedType;
use graphox_core::document::DocumentState;
use lsp_types::*;
use tree_sitter::Node;

#[allow(clippy::too_many_arguments)]
pub(super) fn validate_selection_set(
    this: &DocumentState,
    selection_set: Node,
    offset: usize,
    parent_type: &ExtendedType,
    ctx: &mut ValidationContext,
    depth: usize,
    parent_response_key: Option<&str>,
    type_name: Option<&str>,
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
                    validate_field(
                        this,
                        inner,
                        offset,
                        parent_type,
                        ctx,
                        depth + 1,
                        parent_response_key,
                        type_name,
                    );
                } else if k == "inline_fragment" {
                    crate::diagnostics::fragments::validate_inline_fragment(
                        this,
                        inner,
                        offset,
                        parent_type,
                        ctx,
                        depth + 1,
                        parent_response_key,
                    );
                } else if k == "fragment_spread" {
                    crate::diagnostics::fragments::validate_fragment_spread(
                        this,
                        inner,
                        offset,
                        ctx,
                        parent_response_key,
                        type_name,
                    );
                }
            }
        } else if kind == "field" {
            validate_field(
                this,
                child,
                offset,
                parent_type,
                ctx,
                depth + 1,
                parent_response_key,
                type_name,
            );
        } else if kind == "fragment_spread" {
            crate::diagnostics::fragments::validate_fragment_spread(
                this,
                child,
                offset,
                ctx,
                parent_response_key,
                type_name,
            );
        } else if kind == "inline_fragment" {
            crate::diagnostics::fragments::validate_inline_fragment(
                this,
                child,
                offset,
                parent_type,
                ctx,
                depth + 1,
                parent_response_key,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn validate_field(
    this: &DocumentState,
    field_node: Node,
    offset: usize,
    parent_type: &ExtendedType,
    ctx: &mut ValidationContext,
    depth: usize,
    parent_response_key: Option<&str>,
    type_name: Option<&str>,
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
        // Determine response key (what the response will contain) and the
        // actual field name (the definition on the type). Lookups must use
        // the actual field name; previously we used the response key which
        // caused fields accessed via aliases to appear missing.
        let response_key = if let Some(alias_node) = alias_node {
            // alias_node likely contains a name child; find it
            let mut a_cursor = alias_node.walk();
            let mut found = None;
            for a_child in alias_node.children(&mut a_cursor) {
                if a_child.kind() == "name" {
                    found = Some(this.get_node_text(a_child, offset));
                    break;
                }
            }
            found.unwrap_or_else(|| this.get_node_text(name_node, offset))
        } else {
            this.get_node_text(name_node, offset)
        };

        let actual_field_name = this.get_node_text(name_node, offset);

        // Check for duplicate field names in the same selection set (shallow check)
        // This rule is configurable via config.rules.no_duplicate_fields
        if let Some(cfg) = ctx.config
            && cfg.rules().no_duplicate_fields()
        {
            // Only consider shallow duplicates within the same selection set level
            if depth >= 1 {
                // Find the parent selection set and our "unit" node (either selection or field)
                let (parent, unit_node) = if let Some(p) = field_node.parent() {
                    if p.kind() == "selection" {
                        if let Some(pp) = p.parent() {
                            (pp, p)
                        } else {
                            (p, field_node)
                        }
                    } else {
                        (p, field_node)
                    }
                } else {
                    // fallback
                    (field_node, field_node)
                };

                if parent.kind() == "selection_set" {
                    let mut seen_conflict = false;
                    let mut cursor = parent.walk();

                    // Get current field's arguments and selection set text (if any)
                    let field_args_text = arguments_node
                        .map(|n| this.get_node_text(n, offset))
                        .unwrap_or_default();
                    let field_sel_text = selection_set_node
                        .map(|n| this.get_node_text(n, offset))
                        .unwrap_or_default();

                    for sibling in parent.children(&mut cursor) {
                        if sibling.id() == unit_node.id() {
                            // stop scanning once we reach ourselves
                            break;
                        }

                        if sibling.kind() == "selection" || sibling.kind() == "field" {
                            // If sibling is a selection, we need to look at its child field
                            let target_node = if sibling.kind() == "selection" {
                                let mut found = None;
                                let mut s_cursor = sibling.walk();
                                for s_child in sibling.children(&mut s_cursor) {
                                    if s_child.kind() == "field" {
                                        found = Some(s_child);
                                        break;
                                    }
                                }
                                found
                            } else {
                                Some(sibling)
                            };

                            if let Some(target) = target_node {
                                // find sibling response key (alias or name)
                                let mut s_cursor = target.walk();
                                let mut sibling_key = None;

                                for s_child in target.children(&mut s_cursor) {
                                    if s_child.kind() == "alias" {
                                        // alias node contains a name child
                                        let mut a_cursor = s_child.walk();
                                        for a_child in s_child.children(&mut a_cursor) {
                                            if a_child.kind() == "name" {
                                                sibling_key =
                                                    Some(this.get_node_text(a_child, offset));
                                                break;
                                            }
                                        }
                                    } else if s_child.kind() == "name" && sibling_key.is_none() {
                                        sibling_key = Some(this.get_node_text(s_child, offset));
                                    }
                                }

                                if let Some(s_key) = sibling_key
                                    && s_key == response_key
                                {
                                    // same response key; treated as duplicate
                                    seen_conflict = true;
                                    break;
                                }
                            }
                        }
                    }

                    if seen_conflict {
                        // Point diagnostic at alias name if it exists, otherwise the field name
                        let diagnostic_node = if let Some(alias) = alias_node {
                            let mut a_cursor = alias.walk();
                            alias
                                .children(&mut a_cursor)
                                .find(|c| c.kind() == "name")
                                .unwrap_or(alias)
                        } else {
                            name_node
                        };
                        ctx.diagnostics.push(Diagnostic {
                            range: this.translate_to_file_range(diagnostic_node, offset),
                            severity: Some(DiagnosticSeverity::ERROR),
                            message: format!("Duplicate field '{}' in selection set", response_key),
                            code: Some(lsp_types::NumberOrString::String(
                                "no_duplicate_fields".to_string(),
                            )),
                            // Attach contextual data so code action can compute a better removal
                            data: Some(serde_json::json!({
                                "response_key": response_key,
                                "args": field_args_text,
                                "selection": field_sel_text,
                            })),
                            source: DIAGNOSTIC_SOURCE.map(String::from),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        // Track selected field for required fields validation
        // Fields are tracked by response key (alias or field name)
        if ctx.is_operation {
            if let Some(rk) = parent_response_key {
                if type_name.is_none() {
                    // Track field under parent response key (for nested fields)
                    ctx.response_key_selected_fields
                        .entry(rk.to_string().into())
                        .or_default()
                        .insert(actual_field_name.clone().into());
                }

                // If we're in an inline fragment context, also track in type_condition_fields
                if let Some(tn) = type_name {
                    ctx.type_condition_fields
                        .entry(rk.to_string().into())
                        .or_default()
                        .entry(tn.to_string().into())
                        .or_default()
                        .insert(actual_field_name.clone().into());
                }
            } else if depth == 1 {
                // Root-level field - track under its own key
                ctx.response_key_selected_fields
                    .entry(response_key.clone().into())
                    .or_default()
                    .insert(actual_field_name.clone().into());
                // Mark this response key as root-level (skip required field validation)
                ctx.root_response_keys.insert(response_key.clone().into());
            }
        }

        if actual_field_name == "__typename" {
            return;
        }

        let field_def = match parent_type {
            ExtendedType::Object(obj) => obj.fields.get(actual_field_name.as_str()),
            ExtendedType::Interface(iface) => iface.fields.get(actual_field_name.as_str()),
            _ => None,
        };

        if let Some(field_def) = field_def {
            if let Some(directive) = field_def.directives.get("deprecated") {
                let reason = directive
                    .argument_by_name("reason", ctx.schema)
                    .ok()
                    .and_then(|arg| arg.as_str())
                    .unwrap_or("No reason provided");

                crate::diagnostics::DocumentDiagnostics::add_deprecation_diagnostic(
                    this,
                    ctx,
                    name_node,
                    offset,
                    format!("Field '{}' is deprecated: {}", actual_field_name, reason),
                    reason,
                );
            }

            if let Some(args_node) = arguments_node {
                crate::diagnostics::values::validate_arguments(
                    this,
                    args_node,
                    offset,
                    &field_def.arguments,
                    ctx,
                );
            }

            if let Some(dirs_node) = directives_node {
                crate::diagnostics::values::validate_directives(this, dirs_node, offset, ctx);
            }

            if let Some(sel_set) = selection_set_node {
                let field_type_name = field_def.ty.inner_named_type();
                if let Some(field_type_def) = ctx.schema.types.get(field_type_name.as_str()) {
                    if ctx.is_operation {
                        ctx.response_key_types
                            .insert(response_key.clone().into(), field_type_def.clone());
                    }
                    // Use this field's response key as parent for nested fields
                    let new_parent_rk = if ctx.is_operation {
                        Some(response_key.as_str())
                    } else {
                        parent_response_key
                    };
                    validate_selection_set(
                        this,
                        sel_set,
                        offset,
                        field_type_def,
                        ctx,
                        depth + 1,
                        new_parent_rk,
                        None,
                    );
                }
            }
        } else {
            // Field not found, but still mark variables in arguments as used to avoid redundant "unused variable" warnings
            if let Some(args_node) = arguments_node {
                crate::diagnostics::values::mark_variables_in_arguments_used(
                    this, args_node, offset, ctx,
                );
            }

            let type_name = parent_type.name();

            // Find similar field names to suggest
            let available_fields: Vec<&str> = match parent_type {
                ExtendedType::Object(obj) => obj.fields.keys().map(|s| s.as_str()).collect(),
                ExtendedType::Interface(iface) => iface.fields.keys().map(|s| s.as_str()).collect(),
                _ => vec![],
            };

            let similar_fields = find_similar_fields(&actual_field_name, &available_fields);

            let message = if similar_fields.is_empty() {
                format!(
                    "Field '{}' not found on type '{}'",
                    actual_field_name, type_name
                )
            } else {
                format!(
                    "Field '{}' not found on type '{}'. Did you mean {}?",
                    actual_field_name,
                    type_name,
                    similar_fields
                        .iter()
                        .map(|f| format!("'{}'", f))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };

            ctx.diagnostics.push(Diagnostic {
                range: this.translate_to_file_range(name_node, offset),
                severity: Some(DiagnosticSeverity::ERROR),
                message,
                code: Some(lsp_types::NumberOrString::String(
                    "missing_field".to_string(),
                )),
                data: Some(serde_json::json!({
                    "similar_fields": similar_fields,
                })),
                source: DIAGNOSTIC_SOURCE.map(String::from),
                ..Default::default()
            });
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
