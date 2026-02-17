use super::ValidationContext;
use apollo_compiler::ast::OperationType;
use apollo_compiler::schema::ExtendedType;
use graphox_core::document::DocumentState;
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
    ctx.response_key_types.clear();

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
        && let Some(operation_type) = &ctx.current_operation_type
    {
        let rules = config.rules();
        let required_fields = rules.required_fields();

        // Find the name node of the operation for the diagnostic range
        let mut cursor = node.walk();
        let name_node = node.children(&mut cursor).find(|c| c.kind() == "name");
        let operation_range = name_node
            .map(|n| this.translate_to_file_range(n, offset))
            .unwrap_or_else(|| this.translate_to_file_range(node, offset));

        for (field_name, rule) in required_fields {
            if !rule.applies_to_operation(operation_type) {
                continue;
            }

            let field_name_str = field_name.as_str();

            // 1. Check root-level required fields (fields on Query/Mutation/Subscription)
            let root_type_name = match operation_type.as_ref() {
                "query" => ctx.schema.root_operation(OperationType::Query),
                "mutation" => ctx.schema.root_operation(OperationType::Mutation),
                "subscription" => ctx.schema.root_operation(OperationType::Subscription),
                _ => None,
            };

            if let Some(rtn) = root_type_name
                && let Some(root_type) = ctx.schema.types.get(rtn.as_str())
            {
                let field_exists_on_root = match root_type {
                    ExtendedType::Object(obj) => obj.fields.contains_key(field_name_str),
                    ExtendedType::Interface(iface) => iface.fields.contains_key(field_name_str),
                    _ => false,
                };

                if field_exists_on_root {
                    // Check if this field was selected at root level
                    let is_selected =
                        ctx.response_key_selected_fields.iter().any(|(rk, fields)| {
                            ctx.root_response_keys.contains(rk) && fields.contains(field_name_str)
                        });

                    if !is_selected {
                        let anchor_node =
                            find_root_selection_anchor_for_response_key(this, node, offset, None);
                        if let Some(anchor) = anchor_node
                            && crate::diagnostics::DocumentDiagnostics::has_inline_ignore_comment(
                                this, anchor, offset,
                            )
                        {
                            continue;
                        }

                        ctx.diagnostics.push(Diagnostic {
                            range: anchor_node
                                .map(|n| this.translate_to_file_range(n, offset))
                                .unwrap_or(operation_range),
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
                    }
                }
            }

            // 2. Check ALL selected fields (recursive check)
            for (response_key, type_def) in &ctx.response_key_types {
                let empty_set = ahash::AHashSet::default();
                let selected_fields = ctx
                    .response_key_selected_fields
                    .get(response_key)
                    .unwrap_or(&empty_set);

                let field_exists = match type_def {
                    ExtendedType::Object(obj) => obj.fields.contains_key(field_name_str),
                    ExtendedType::Interface(iface) => iface.fields.contains_key(field_name_str),
                    _ => false,
                };

                if field_exists {
                    let mut is_selected = selected_fields.contains(field_name_str);

                    // For object types, fields selected in an inline fragment on the same type also count
                    if !is_selected
                        && let ExtendedType::Object(obj) = type_def
                        && let Some(type_fields) = ctx.type_condition_fields.get(response_key)
                        && let Some(fields) = type_fields.get(obj.name.as_str())
                    {
                        is_selected = fields.contains(field_name_str);
                    }

                    if !is_selected {
                        let anchor_node = find_root_selection_anchor_for_response_key(
                            this,
                            node,
                            offset,
                            Some(response_key.as_ref()),
                        );
                        if let Some(anchor) = anchor_node
                            && crate::diagnostics::DocumentDiagnostics::has_inline_ignore_comment(
                                this, anchor, offset,
                            )
                        {
                            continue;
                        }

                        ctx.diagnostics.push(Diagnostic {
                            range: anchor_node
                                .map(|n| this.translate_to_file_range(n, offset))
                                .unwrap_or(operation_range),
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

            // 3. Check inline fragment type conditions (merging base selections)
            for (response_key, type_conditions) in &ctx.response_key_type_conditions {
                let base_selected_fields = ctx.response_key_selected_fields.get(response_key);

                for type_name in type_conditions {
                    let type_fields = ctx
                        .type_condition_fields
                        .get(response_key)
                        .and_then(|m| m.get(type_name));

                    let type_name_str = type_name.to_string();
                    if let Some(type_def) = ctx.schema.types.get(&*type_name_str) {
                        let field_exists = match type_def {
                            ExtendedType::Object(obj) => obj.fields.contains_key(field_name_str),
                            ExtendedType::Interface(iface) => {
                                iface.fields.contains_key(field_name_str)
                            }
                            _ => false,
                        };

                        if field_exists {
                            let is_selected = type_fields
                                .is_some_and(|f| f.contains(field_name_str))
                                || base_selected_fields.is_some_and(|f| f.contains(field_name_str));

                            if !is_selected {
                                let anchor_node = find_root_selection_anchor_for_response_key(
                                    this,
                                    node,
                                    offset,
                                    Some(response_key.as_ref()),
                                );
                                if let Some(anchor) = anchor_node
                                    && crate::diagnostics::DocumentDiagnostics::has_inline_ignore_comment(
                                        this, anchor, offset,
                                    )
                                {
                                    continue;
                                }

                                ctx.diagnostics.push(Diagnostic {
                                    range: anchor_node
                                        .map(|n| this.translate_to_file_range(n, offset))
                                        .unwrap_or(operation_range),
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
}

fn find_root_selection_anchor_for_response_key<'a>(
    this: &DocumentState,
    operation_node: Node<'a>,
    offset: usize,
    response_key: Option<&str>,
) -> Option<Node<'a>> {
    let mut op_cursor = operation_node.walk();
    for child in operation_node.children(&mut op_cursor) {
        if child.kind() != "selection_set" {
            continue;
        }

        let mut sel_cursor = child.walk();
        for selection in child.children(&mut sel_cursor) {
            let field_node = if selection.kind() == "selection" {
                this.find_child_by_kind(selection, "field")
            } else if selection.kind() == "field" {
                Some(selection)
            } else {
                None
            };

            let Some(field) = field_node else {
                continue;
            };

            let components = this.extract_field_components(field);
            let Some(name_node) = components.name else {
                continue;
            };

            let mut key = this.get_node_text(name_node, offset);
            let mut anchor_node = name_node;
            if let Some(alias_node) = components.alias
                && let Some(alias_name_node) = this.find_child_by_kind(alias_node, "name")
            {
                key = this.get_node_text(alias_name_node, offset);
                anchor_node = alias_name_node;
            }

            if response_key.is_none() || response_key == Some(key.as_str()) {
                return Some(anchor_node);
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
