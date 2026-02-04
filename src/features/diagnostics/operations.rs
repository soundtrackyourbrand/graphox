use super::ValidationContext;
use crate::document::DocumentState;
use apollo_compiler::ast::OperationType;
use tower_lsp::lsp_types::*;
use tree_sitter::Node;

impl DocumentState {
    pub(super) fn validate_operation(
        &self,
        node: Node,
        offset: usize,
        ctx: &mut ValidationContext,
    ) {
        ctx.used_variables.clear();
        ctx.defined_variables.clear();

        let mut operation_type_string = String::from("query");
        let mut cursor = node.walk();
        let mut var_defs_node = None;

        for child in node.children(&mut cursor) {
            if child.kind() == "operation_type" {
                operation_type_string = self.get_node_text(child, offset);
            } else if child.kind() == "variable_definitions" {
                var_defs_node = Some(child);
            }
        }

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
                                    var_name = Some(self.get_node_text(n_child, offset));
                                }
                            }
                        } else if v_child.kind() == "type" {
                            self.validate_type_node(v_child, offset, ctx);
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
                    self.validate_selection_set(child, offset, root_type, ctx);
                }
            }
        }

        // 3. Check for unused variables
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
                                            && self.get_node_text(n_child, offset) == *name
                                        {
                                            ctx.diagnostics.push(Diagnostic {
                                                range: self.translate_to_file_range(child, offset),
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

    pub(super) fn validate_type_node(
        &self,
        node: Node,
        offset: usize,
        ctx: &mut ValidationContext,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "named_type" => {
                    let type_name = self.get_node_text(child, offset);
                    if let Some(type_def) = ctx.schema.types.get(type_name.as_str()) {
                        let directives = type_def.directives();

                        if let Some(directive) = directives.get("deprecated") {
                            let reason = directive
                                .argument_by_name("reason", ctx.schema)
                                .ok()
                                .and_then(|arg| arg.as_str())
                                .unwrap_or("No reason provided");

                            self.add_deprecation_diagnostic(
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
                    self.validate_type_node(child, offset, ctx);
                }
                _ => {}
            }
        }
    }
}
