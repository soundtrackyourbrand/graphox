use super::ValidationContext;
use apollo_compiler::Schema;
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
    ctx.selected_fields.clear();

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
    ctx.current_operation_type = Some(operation_type_string.clone());

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
                    ctx.defined_variables.insert(name);
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
                    this, child, offset, root_type, ctx, 0,
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
                                            && this.get_node_text(n_child, offset) == *name
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

fn field_exists_in_root_type(
    schema: &apollo_compiler::validation::Valid<Schema>,
    operation_type: &str,
    field_name: &str,
) -> bool {
    use apollo_compiler::ast::OperationType;

    let op_type = match operation_type {
        "query" => OperationType::Query,
        "mutation" => OperationType::Mutation,
        "subscription" => OperationType::Subscription,
        _ => return false,
    };

    if let Some(root_def_name) = schema.root_operation(op_type)
        && let Some(root_type) = schema.types.get(root_def_name.as_str())
    {
        let field_def = match root_type {
            ExtendedType::Object(obj) => obj.fields.get(field_name),
            ExtendedType::Interface(iface) => iface.fields.get(field_name),
            _ => None,
        };
        return field_def.is_some();
    }
    false
}

pub(super) fn check_required_fields(
    this: &DocumentState,
    node: Node,
    offset: usize,
    ctx: &mut ValidationContext,
) {
    // Only check if we have a config and rules
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

        // Check each required field
        for (field_name, rule) in required_fields {
            // Check if this rule applies to the current operation type
            if rule.applies_to_operation(operation_type) {
                // Check if the field exists on the root type
                let field_exists_on_root =
                    field_exists_in_root_type(ctx.schema, operation_type, field_name);

                // If the field doesn't exist on the root type, skip the check
                // because it's either a nested field or a non-existent field
                // - Nested fields: user would get apollo-compiler error for undefined field
                // - Non-existent fields: user would get apollo-compiler error for undefined field
                if !field_exists_on_root {
                    continue;
                }

                // Check if the field was selected
                if !ctx.selected_fields.contains(field_name) {
                    ctx.diagnostics.push(Diagnostic {
                        range,
                        severity: Some(DiagnosticSeverity::ERROR),
                        message: format!(
                            "Required field '{}' must be selected in {} operations",
                            field_name, operation_type
                        ),
                        code: Some(NumberOrString::String("required_field_missing".to_string())),
                        ..Default::default()
                    });
                }
            }
        }
    }
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
