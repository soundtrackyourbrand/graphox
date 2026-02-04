use super::ValidationContext;
use crate::document::DocumentState;
use apollo_compiler::schema::{ExtendedType, FieldDefinition};
use tower_lsp::lsp_types::*;
use tree_sitter::Node;

impl DocumentState {
    pub(super) fn validate_arguments(
        &self,
        node: Node,
        offset: usize,
        field_def: &FieldDefinition,
        ctx: &mut ValidationContext,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "argument" {
                let mut arg_cursor = child.walk();
                let mut name_node = None;
                let mut value_node = None;
                for arg_child in child.children(&mut arg_cursor) {
                    if arg_child.kind() == "name" {
                        name_node = Some(arg_child);
                    } else if arg_child.kind().ends_with("_value") || arg_child.kind() == "value" {
                        value_node = Some(arg_child);
                    }
                }

                if let Some(name_node) = name_node {
                    let arg_name = self.get_node_text(name_node, offset);
                    if let Some(arg_def) = field_def
                        .arguments
                        .iter()
                        .find(|a| a.name.as_str() == arg_name)
                    {
                        if let Some(directive) = arg_def.directives.get("deprecated") {
                            let reason = directive
                                .argument_by_name("reason", ctx.schema)
                                .ok()
                                .and_then(|arg| arg.as_str())
                                .unwrap_or("No reason provided");

                            self.add_deprecation_diagnostic(
                                ctx,
                                name_node,
                                offset,
                                format!("Argument '{}' is deprecated: {}", arg_name, reason),
                                reason,
                            );
                        }

                        if let Some(v_node) = value_node {
                            let arg_type_name = arg_def.ty.inner_named_type();
                            if let Some(arg_type_def) = ctx.schema.types.get(arg_type_name.as_str())
                            {
                                self.validate_value(v_node, offset, arg_type_def, ctx);
                            }
                        }
                    }
                }
            }
        }
    }

    pub(super) fn validate_value(
        &self,
        node: Node,
        offset: usize,
        expected_type: &ExtendedType,
        ctx: &mut ValidationContext,
    ) {
        match node.kind() {
            "value" => {
                if let Some(child) = node.child(0) {
                    self.validate_value(child, offset, expected_type, ctx);
                }
            }
            "variable" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "name" {
                        let name = self.get_node_text(child, offset);
                        ctx.used_variables.insert(name.clone());

                        if !ctx.defined_variables.contains(&name) && ctx.workspace_loaded {
                            ctx.diagnostics.push(Diagnostic {
                                range: self.translate_to_file_range(node, offset),
                                severity: Some(DiagnosticSeverity::ERROR),
                                message: format!("Undefined variable: ${}", name),
                                code: Some(NumberOrString::String(
                                    "undefined_variable".to_string(),
                                )),
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
                            let mut field_cursor = child.walk();
                            let mut name_node = None;
                            let mut value_node = None;
                            for field_child in child.children(&mut field_cursor) {
                                if field_child.kind() == "name" {
                                    name_node = Some(field_child);
                                } else if field_child.kind().ends_with("_value")
                                    || field_child.kind() == "value"
                                {
                                    value_node = Some(field_child);
                                }
                            }

                            if let Some(name_node) = name_node {
                                let field_name = self.get_node_text(name_node, offset);
                                if let Some(field_def) = input_obj.fields.get(field_name.as_str()) {
                                    if let Some(directive) = field_def.directives.get("deprecated")
                                    {
                                        let reason = directive
                                            .argument_by_name("reason", ctx.schema)
                                            .ok()
                                            .and_then(|arg| arg.as_str())
                                            .unwrap_or("No reason provided");

                                        self.add_deprecation_diagnostic(
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

                                    if let Some(v_node) = value_node {
                                        let field_type_name = field_def.ty.inner_named_type();
                                        if let Some(field_type_def) =
                                            ctx.schema.types.get(field_type_name.as_str())
                                        {
                                            self.validate_value(
                                                v_node,
                                                offset,
                                                field_type_def,
                                                ctx,
                                            );
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
                        self.validate_value(child, offset, expected_type, ctx);
                    }
                }
            }
            "enum_value" => {
                if let ExtendedType::Enum(enum_def) = expected_type {
                    let value_name = self.get_node_text(node, offset);
                    if let Some(value_def) = enum_def.values.get(value_name.as_str())
                        && let Some(directive) = value_def.directives.get("deprecated")
                    {
                        let reason = directive
                            .argument_by_name("reason", ctx.schema)
                            .ok()
                            .and_then(|arg| arg.as_str())
                            .unwrap_or("No reason provided");

                        self.add_deprecation_diagnostic(
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
}
