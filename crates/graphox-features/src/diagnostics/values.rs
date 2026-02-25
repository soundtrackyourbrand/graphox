use super::DIAGNOSTIC_SOURCE;
use super::ValidationContext;
use apollo_compiler::schema::{ExtendedType, InputValueDefinition};
use graphox_core::document::DocumentState;
use lsp_types::*;
use tree_sitter::Node;

pub(super) fn validate_arguments(
    this: &DocumentState,
    node: Node,
    offset: usize,
    arg_defs: &[apollo_compiler::Node<InputValueDefinition>],
    ctx: &mut ValidationContext,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "argument" {
            let components = this.extract_named_value_components(child);

            if let Some(name_node) = components.name {
                let arg_name = this.get_node_text(name_node, offset);
                if let Some(arg_def) = arg_defs.iter().find(|a| a.name.as_str() == arg_name) {
                    if let Some(directive) = arg_def.directives.get("deprecated") {
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
                            format!("Argument '{}' is deprecated: {}", arg_name, reason),
                            reason,
                        );
                    }

                    if let Some(v_node) = components.value {
                        let arg_type_name = arg_def.ty.inner_named_type();
                        if let Some(arg_type_def) = ctx.schema.types.get(arg_type_name.as_str()) {
                            validate_value(this, v_node, offset, arg_type_def, ctx);
                        }
                    }
                } else {
                    // Argument not found in definition, but still mark variables as used to avoid
                    // redundant "unused variable" warnings.
                    if let Some(v_node) = components.value {
                        mark_variables_used(this, v_node, offset, ctx);
                    }
                }
            }
        }
    }
}

pub(super) fn validate_directives(
    this: &DocumentState,
    node: Node,
    offset: usize,
    ctx: &mut ValidationContext,
) {
    if node.kind() == "directive" {
        validate_directive_node(this, node, offset, ctx);
        return;
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "directives" {
            validate_directives(this, child, offset, ctx);
        } else if child.kind() == "directive" {
            validate_directive_node(this, child, offset, ctx);
        }
    }
}

fn validate_directive_node(
    this: &DocumentState,
    node: Node,
    offset: usize,
    ctx: &mut ValidationContext,
) {
    let mut dir_cursor = node.walk();
    let mut name_node = None;
    let mut arguments_node = None;
    for dir_child in node.children(&mut dir_cursor) {
        if dir_child.kind() == "name" {
            name_node = Some(dir_child);
        } else if dir_child.kind() == "arguments" {
            arguments_node = Some(dir_child);
        }
    }

    if let Some(name_node) = name_node {
        let dir_name = this.get_node_text(name_node, offset);
        if let Some(dir_def) = ctx.schema.directive_definitions.get(dir_name.as_str())
            && let Some(args_node) = arguments_node
        {
            validate_arguments(this, args_node, offset, &dir_def.arguments, ctx);
        }
    }
}

use std::sync::Arc;
pub(super) fn validate_value(
    this: &DocumentState,
    node: Node,
    offset: usize,
    expected_type: &ExtendedType,
    ctx: &mut ValidationContext,
) {
    match node.kind() {
        "value" => {
            if let Some(child) = node.child(0) {
                validate_value(this, child, offset, expected_type, ctx);
            }
        }
        "variable" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "name" {
                    let name = this.get_node_text(child, offset);
                    let name_arc: Arc<str> = name.clone().into();
                    ctx.used_variables.insert(name_arc.clone());

                    if ctx.is_operation
                        && !ctx.defined_variables.contains(name_arc.as_ref())
                        && ctx.workspace_loaded
                    {
                        ctx.diagnostics.push(Diagnostic {
                            range: this.translate_to_file_range(node, offset),
                            severity: Some(DiagnosticSeverity::ERROR),
                            message: format!("Undefined variable: ${}", name),
                            code: Some(NumberOrString::String("undefined_variable".to_string())),
                            source: DIAGNOSTIC_SOURCE.map(String::from),
                            ..Default::default()
                        });
                    }
                }
            }
        }
        "object_value" => {
            if let ExtendedType::InputObject(input_obj) = expected_type {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "object_field" {
                        let components = this.extract_named_value_components(child);

                        if let Some(name_node) = components.name {
                            let field_name = this.get_node_text(name_node, offset);
                            if let Some(field_def) = input_obj.fields.get(field_name.as_str()) {
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
                                        format!(
                                            "Input field '{}' is deprecated: {}",
                                            field_name, reason
                                        ),
                                        reason,
                                    );
                                }

                                if let Some(v_node) = components.value {
                                    let field_type_name = field_def.ty.inner_named_type();
                                    if let Some(field_type_def) =
                                        ctx.schema.types.get(field_type_name.as_str())
                                    {
                                        validate_value(this, v_node, offset, field_type_def, ctx);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        "list_value" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind().ends_with("_value") || child.kind() == "value" {
                    validate_value(this, child, offset, expected_type, ctx);
                }
            }
        }
        "enum_value" => {
            if let ExtendedType::Enum(enum_def) = expected_type {
                let value_name = this.get_node_text(node, offset);
                if let Some(value_def) = enum_def.values.get(value_name.as_str())
                    && let Some(directive) = value_def.directives.get("deprecated")
                {
                    let reason = directive
                        .argument_by_name("reason", ctx.schema)
                        .ok()
                        .and_then(|arg| arg.as_str())
                        .unwrap_or("No reason provided");

                    crate::diagnostics::DocumentDiagnostics::add_deprecation_diagnostic(
                        this,
                        ctx,
                        node,
                        offset,
                        format!("Enum value '{}' is deprecated: {}", value_name, reason),
                        reason,
                    );
                }
            }
        }
        _ => {}
    }
}

pub(super) fn mark_variables_in_arguments_used(
    this: &DocumentState,
    node: Node,
    offset: usize,
    ctx: &mut ValidationContext,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "argument" {
            let components = this.extract_named_value_components(child);
            if let Some(v_node) = components.value {
                mark_variables_used(this, v_node, offset, ctx);
            }
        }
    }
}

pub(super) fn mark_variables_used(
    this: &DocumentState,
    node: Node,
    offset: usize,
    ctx: &mut ValidationContext,
) {
    match node.kind() {
        "value" => {
            if let Some(child) = node.child(0) {
                mark_variables_used(this, child, offset, ctx);
            }
        }
        "variable" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "name" {
                    let name = this.get_node_text(child, offset);
                    ctx.used_variables.insert(name.into());
                }
            }
        }
        "object_value" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "object_field" {
                    let components = this.extract_named_value_components(child);
                    if let Some(v_node) = components.value {
                        mark_variables_used(this, v_node, offset, ctx);
                    }
                }
            }
        }
        "list_value" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind().ends_with("_value") || child.kind() == "value" {
                    mark_variables_used(this, child, offset, ctx);
                }
            }
        }
        _ => {}
    }
}
