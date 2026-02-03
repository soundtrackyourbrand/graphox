use super::ValidationContext;
use crate::document::DocumentState;
use apollo_compiler::ast::OperationType;
use tree_sitter::Node;

impl DocumentState {
    pub(super) fn validate_operation(
        &self,
        node: Node,
        offset: usize,
        ctx: &mut ValidationContext,
    ) {
        let mut operation_type_string = String::from("query");
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if child.kind() == "operation_type" {
                operation_type_string = self.get_node_text(child, offset);
            } else if child.kind() == "variable_definitions" {
                self.validate_variable_definitions(child, offset, ctx);
            }
        }

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
    }

    pub(super) fn validate_variable_definitions(
        &self,
        node: Node,
        offset: usize,
        ctx: &mut ValidationContext,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "variable_definition" {
                let mut vd_cursor = child.walk();
                for vd_child in child.children(&mut vd_cursor) {
                    if vd_child.kind() == "type" {
                        self.validate_type_node(vd_child, offset, ctx);
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
