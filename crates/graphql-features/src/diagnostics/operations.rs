use super::ValidationContext;
use apollo_compiler::ast::OperationType;
use apollo_compiler::schema::ExtendedType;
use graphql_core::document::DocumentState;
use lsp_types::*;
use tree_sitter::Node;

pub(super) fn validate_operation(
    this: &DocumentState,
    node: Node,
    offset: usize,
    ctx: &mut ValidationContext,
) {
    ctx.is_operation = true;
    ctx.used_variables.clear();
    ctx.defined_variables.clear();
    ctx.response_key_selected_fields.clear();
    ctx.response_key_type_conditions.clear();
    ctx.type_condition_fields.clear();
    ctx.root_response_keys.clear();

    let mut operation_type_string = String::from("query");
    let mut cursor = node.walk();
    let mut var_defs_node = None;

    for child in node.children(&mut cursor) {
        if child.kind() == "operation_type" {
            operation_type_string = this.get_node_text(child, offset);
        } else if child.kind() == "variable_definitions" {
            var_defs_node = Some(child);
        }
    }

    // Set the current operation type
    ctx.current_operation_type = Some(operation_type_string.clone().into());

    // 1. Collect and validate variable definitions
    if let Some(var_defs) = var_defs_node {
        let mut vd_cursor = var_defs.walk();
        for child in var_defs.children(&mut vd_cursor) {
            if child.kind() == "variable_definition" {
                let mut v_cursor = child.walk();
                let mut var_name = None;
                for v_child in child.children(&mut v_cursor) {
                    if v_child.kind() == "variable" {
                        let mut n_cursor = v_child.walk();
                        for n_child in v_child.children(&mut n_cursor) {
                            if n_child.kind() == "name" {
                                var_name = Some(this.get_node_text(n_child, offset));
                            }
                        }
                    } else if v_child.kind() == "type" {
                        validate_type_node(this, v_child, offset, ctx);
                    }
                }

                if let Some(name) = var_name {
                    ctx.defined_variables.insert(name.into());
                }
            }
        }
    }

    // 2. Collect used variables in selection set
    let op_type = match operation_type_string.as_str() {
        "query" => Some(OperationType::Query),
        "mutation" => Some(OperationType::Mutation),
        "subscription" => Some(OperationType::Subscription),
        _ => None,
    };

    if let Some(op) = op_type
        && let Some(root_def_name) = ctx.schema.root_operation(op)
        && let Some(root_type) = ctx.schema.types.get(root_def_name.as_str())
    {
        for child in node.children(&mut cursor) {
            if child.kind() == "selection_set" {
                crate::diagnostics::selection_set::validate_selection_set(
                    this, child, offset, root_type, ctx, 0, None, None,
                );
            }
        }
    }

    // Check required fields after validating the selection set
    check_required_fields(this, node, offset, ctx);

    // 3. Check for unused variables
    if ctx.workspace_loaded {
        for name in &ctx.defined_variables {
            if !ctx.used_variables.contains(name) {
                // Find the node for this variable definition to report the diagnostic on it
                if let Some(var_defs) = var_defs_node {
                    let mut vd_cursor = var_defs.walk();
                    for child in var_defs.children(&mut vd_cursor) {
                        if child.kind() == "variable_definition" {
                            let mut v_cursor = child.walk();
                            for v_child in child.children(&mut v_cursor) {
                                if v_child.kind() == "variable" {
                                    let mut n_cursor = v_child.walk();
                                    for n_child in v_child.children(&mut n_cursor) {
                                        if n_child.kind() == "name"
                                            && this.get_node_text(n_child, offset) == name.as_ref()
                                        {
                                            ctx.diagnostics.push(Diagnostic {
                                                range: this
                                                    .translate_to_file_range(v_child, offset),
                                                severity: Some(DiagnosticSeverity::WARNING),
                                                message: format!("Unused variable: ${}", name),
                                                code: Some(NumberOrString::String(
                                                    "unused_variable".to_string(),
                                                )),
                                                tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                                                ..Default::default()
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub(super) fn check_required_fields(
    this: &DocumentState,
    node: Node,
    offset: usize,
    ctx: &mut ValidationContext,
) {
    if let Some(config) = ctx.config
        && let Some(rules) = &config.rules
        && let Some(required_fields) = &rules.required_fields
        && let Some(operation_type) = &ctx.current_operation_type
    {
        // Find the name node of the operation for the diagnostic range
        let mut cursor = node.walk();
        let name_node = node.children(&mut cursor).find(|c| c.kind() == "name");
        let range = name_node
            .map(|n| this.translate_to_file_range(n, offset))
            .unwrap_or_else(|| this.translate_to_file_range(node, offset));

        for (field_name, rule) in required_fields {
            if !rule.applies_to_operation(operation_type) {
                continue;
            }

            let field_name_str = field_name.as_str();

            // 1. Check root-level required fields (fields on Query/Mutation/Subscription)
            // If the required field exists on root type, check if it was selected
            let root_type_name = match operation_type.as_ref() {
                "query" => ctx.schema.root_operation(OperationType::Query),
                "mutation" => ctx.schema.root_operation(OperationType::Mutation),
                "subscription" => ctx.schema.root_operation(OperationType::Subscription),
                _ => None,
            };

            let field_selected_at_root = if let Some(rtn) = root_type_name {
                if let Some(root_type) = ctx.schema.types.get(rtn.as_str()) {
                    let field_exists_on_root = match root_type {
                        ExtendedType::Object(obj) => obj.fields.contains_key(field_name_str),
                        ExtendedType::Interface(iface) => iface.fields.contains_key(field_name_str),
                        _ => false,
                    };

                    if field_exists_on_root {
                        // Check if this field was selected at root level
                        ctx.response_key_selected_fields.iter().any(|(rk, fields)| {
                            ctx.root_response_keys.contains(rk) && fields.contains(field_name_str)
                        })
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };

            if !field_selected_at_root {
                // For root-level fields that exist but weren't selected, report error
                // (This handles the original behavior)
                let root_type_name_check = match operation_type.as_ref() {
                    "query" => ctx.schema.root_operation(OperationType::Query),
                    "mutation" => ctx.schema.root_operation(OperationType::Mutation),
                    "subscription" => ctx.schema.root_operation(OperationType::Subscription),
                    _ => None,
                };

                if let Some(rtn) = root_type_name_check
                    && let Some(root_type) = ctx.schema.types.get(rtn.as_str()) {
                        let field_exists_on_root = match root_type {
                            ExtendedType::Object(obj) => obj.fields.contains_key(field_name_str),
                            ExtendedType::Interface(iface) => {
                                iface.fields.contains_key(field_name_str)
                            }
                            _ => false,
                        };

                        if field_exists_on_root {
                            ctx.diagnostics.push(Diagnostic {
                                range,
                                severity: Some(DiagnosticSeverity::ERROR),
                                message: format!(
                                    "Required field '{}' must be selected in {} operations",
                                    field_name, operation_type
                                ),
                                code: Some(NumberOrString::String(
                                    "required_field_missing".to_string(),
                                )),
                                ..Default::default()
                            });
                            continue; // Move to next required field
                        }
                    }
            }

            // 2. Check nested fields (non-root level)
            // For each selected root-level field, check if its return type has this required field
            for (response_key, selected_fields) in &ctx.response_key_selected_fields {
                // Only check root-level response keys
                if !ctx.root_response_keys.contains(response_key) {
                    continue;
                }

                // Skip if this response key has inline fragments (step 3 handles those)
                if ctx.response_key_type_conditions.contains_key(response_key) {
                    continue;
                }

                // Find the return type for this response key
                if let Some(return_type) = find_return_type_for_response_key(
                    this,
                    node,
                    offset,
                    response_key.as_ref(),
                    operation_type.as_ref(),
                    ctx,
                )
                    && let Some(type_def) = ctx.schema.types.get(return_type.as_str()) {
                        let field_exists = match type_def {
                            ExtendedType::Object(obj) => obj.fields.contains_key(field_name_str),
                            ExtendedType::Interface(iface) => {
                                iface.fields.contains_key(field_name_str)
                            }
                            _ => false,
                        };

                        if field_exists && !selected_fields.contains(field_name_str) {
                            ctx.diagnostics.push(Diagnostic {
                                range,
                                severity: Some(DiagnosticSeverity::ERROR),
                                message: format!(
                                    "Required field '{}' must be selected in '{}'",
                                    field_name, response_key
                                ),
                                code: Some(NumberOrString::String(
                                    "required_field_missing".to_string(),
                                )),
                                ..Default::default()
                            });
                        }
                    }
            }

            // 3. Check inline fragment type conditions
            for (response_key, type_conditions) in &ctx.response_key_type_conditions {
                for type_name in type_conditions {
                    let type_fields = ctx
                        .type_condition_fields
                        .get(response_key)
                        .and_then(|m| m.get(type_name))
                        .cloned()
                        .unwrap_or_default();

                    let type_name_str = type_name.to_string();
                    if let Some(type_def) = ctx.schema.types.get(&*type_name_str) {
                        let field_exists = match type_def {
                            ExtendedType::Object(obj) => obj.fields.contains_key(field_name_str),
                            ExtendedType::Interface(iface) => {
                                iface.fields.contains_key(field_name_str)
                            }
                            _ => false,
                        };

                        if field_exists && !type_fields.contains(field_name_str) {
                            ctx.diagnostics.push(Diagnostic {
                                range,
                                severity: Some(DiagnosticSeverity::ERROR),
                                message: format!(
                                    "Required field '{}' must be selected in '... on {}'",
                                    field_name, type_name
                                ),
                                code: Some(NumberOrString::String(
                                    "required_field_missing".to_string(),
                                )),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
        }
    }
}

fn find_return_type_for_response_key(
    this: &DocumentState,
    node: Node,
    offset: usize,
    response_key: &str,
    operation_type: &str,
    ctx: &ValidationContext,
) -> Option<String> {
    use apollo_compiler::ast::OperationType;

    let op_type = match operation_type {
        "query" => OperationType::Query,
        "mutation" => OperationType::Mutation,
        "subscription" => OperationType::Subscription,
        _ => return None,
    };

    if let Some(root_def_name) = ctx.schema.root_operation(op_type)
        && let Some(root_type) = ctx.schema.types.get(root_def_name.as_str()) {
            return find_field_type_recursive(this, node, offset, response_key, root_type, ctx);
        }
    None
}

fn find_field_type_recursive(
    this: &DocumentState,
    node: Node,
    offset: usize,
    target_response_key: &str,
    current_type: &ExtendedType,
    ctx: &ValidationContext,
) -> Option<String> {
    // Check if current type has the field we're looking for
    let field_def = match current_type {
        ExtendedType::Object(obj) => obj.fields.get(target_response_key),
        ExtendedType::Interface(iface) => iface.fields.get(target_response_key),
        _ => None,
    };

    if let Some(fdef) = field_def {
        return Some(fdef.ty.inner_named_type().to_string());
    }

    // Search recursively in selection sets
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "selection_set"
            && let Some(found) =
                search_in_selection_set(this, child, offset, target_response_key, current_type, ctx)
            {
                return Some(found);
            }
    }

    None
}

fn search_in_selection_set(
    this: &DocumentState,
    selection_set: Node,
    offset: usize,
    target_response_key: &str,
    parent_type: &ExtendedType,
    ctx: &ValidationContext,
) -> Option<String> {
    let mut cursor = selection_set.walk();
    for child in selection_set.children(&mut cursor) {
        if child.kind() == "selection" || child.kind() == "field" {
            let field_node = if child.kind() == "selection" {
                let mut inner_cursor = child.walk();
                child
                    .children(&mut inner_cursor)
                    .find(|c| c.kind() == "field")
            } else {
                Some(child)
            };

            if let Some(fnode) = field_node {
                let mut f_cursor = fnode.walk();
                let mut response_key = None;
                let mut actual_name = None;
                let mut selection_set_node = None;

                for fchild in fnode.children(&mut f_cursor) {
                    if fchild.kind() == "alias" {
                        let mut a_cursor = fchild.walk();
                        for achild in fchild.children(&mut a_cursor) {
                            if achild.kind() == "name" {
                                response_key = Some(this.get_node_text(achild, offset));
                                break;
                            }
                        }
                    } else if fchild.kind() == "name" {
                        actual_name = Some(this.get_node_text(fchild, offset));
                    } else if fchild.kind() == "selection_set" {
                        selection_set_node = Some(fchild);
                    }
                }

                if actual_name.is_none() {
                    continue;
                }

                let rk = response_key
                    .as_ref()
                    .unwrap_or(actual_name.as_ref().unwrap())
                    .as_str();

                if rk == target_response_key {
                    // Found the field, return its type
                    // Use actual_name (not rk/response_key) to look up the field definition
                    let field_name = actual_name.as_ref().unwrap().as_str();
                    let fdef = match parent_type {
                        ExtendedType::Object(obj) => obj.fields.get(field_name),
                        ExtendedType::Interface(iface) => iface.fields.get(field_name),
                        _ => None,
                    };

                    if let Some(f) = fdef {
                        return Some(f.ty.inner_named_type().to_string());
                    }
                }

                // Continue searching in nested selection sets
                if let Some(sel_set) = selection_set_node {
                    // Use actual_name (not rk/response_key) to look up the field definition
                    let field_name = actual_name.as_ref().unwrap().as_str();
                    let fdef = match parent_type {
                        ExtendedType::Object(obj) => obj.fields.get(field_name),
                        ExtendedType::Interface(iface) => iface.fields.get(field_name),
                        _ => None,
                    };

                    if let Some(field_def) = fdef {
                        let return_type = field_def.ty.inner_named_type();
                        if let Some(return_type_def) = ctx.schema.types.get(return_type.as_str())
                            && let Some(found) = search_in_selection_set(
                                this,
                                sel_set,
                                offset,
                                target_response_key,
                                return_type_def,
                                ctx,
                            ) {
                                return Some(found);
                            }
                    }
                }
            }
        } else if child.kind() == "inline_fragment" {
            // Handle inline fragments
            let mut frag_cursor = child.walk();
            let mut type_name = None;
            let mut sel_set_node = None;

            for fchild in child.children(&mut frag_cursor) {
                if fchild.kind() == "type_condition" {
                    let mut tc_cursor = fchild.walk();
                    for tc_child in fchild.children(&mut tc_cursor) {
                        if tc_child.kind() == "named_type" {
                            let mut nt_cursor = tc_child.walk();
                            for nt_child in tc_child.children(&mut nt_cursor) {
                                if nt_child.kind() == "name" {
                                    type_name = Some(this.get_node_text(nt_child, offset));
                                    break;
                                }
                            }
                        }
                    }
                } else if fchild.kind() == "selection_set" {
                    sel_set_node = Some(fchild);
                }
            }

            if let (Some(tname), Some(sel_set)) = (type_name, sel_set_node)
                && let Some(type_def) = ctx.schema.types.get(tname.as_str())
                    && let Some(found) = search_in_selection_set(
                        this,
                        sel_set,
                        offset,
                        target_response_key,
                        type_def,
                        ctx,
                    ) {
                        return Some(found);
                    }
        }
    }

    None
}

pub(super) fn validate_type_node(
    this: &DocumentState,
    root: Node,
    offset: usize,
    ctx: &mut ValidationContext,
) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "named_type" => {
                    let type_name = this.get_node_text(child, offset);
                    if let Some(type_def) = ctx.schema.types.get(type_name.as_str()) {
                        let directives = type_def.directives();

                        if let Some(directive) = directives.get("deprecated") {
                            let reason = directive
                                .argument_by_name("reason", ctx.schema)
                                .ok()
                                .and_then(|arg| arg.as_str())
                                .unwrap_or("No reason provided");

                            crate::diagnostics::DocumentDiagnostics::add_deprecation_diagnostic(
                                this,
                                ctx,
                                child,
                                offset,
                                format!("Type '{}' is deprecated: {}", type_name, reason),
                                reason,
                            );
                        }
                    }
                }
                "list_type" | "non_null_type" => {
                    stack.push(child);
                }
                _ => {}
            }
        }
    }
}
