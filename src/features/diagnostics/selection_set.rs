use crate::document::DocumentState;
use apollo_compiler::{schema, Schema};
use tower_lsp::lsp_types::*;
use tree_sitter::Node;

impl DocumentState {
    pub(super) fn validate_selection_set(
        &self,
        selection_set: Node,
        offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        all_fragments: &[String],
        diagnostics: &mut Vec<Diagnostic>,
        config: Option<&crate::Config>,
    ) {
        let mut cursor = selection_set.walk();
        for child in selection_set.children(&mut cursor) {
            let kind = child.kind();

            if kind == "selection" {
                let mut inner_cursor = child.walk();
                for inner in child.children(&mut inner_cursor) {
                    let k = inner.kind();
                    if k == "field" {
                        self.validate_field(
                            inner,
                            offset,
                            parent_type,
                            schema,
                            all_fragments,
                            diagnostics,
                            config,
                        );
                    } else if k == "inline_fragment" {
                        self.validate_inline_fragment(
                            inner,
                            offset,
                            parent_type,
                            schema,
                            all_fragments,
                            diagnostics,
                            config,
                        );
                    } else if k == "fragment_spread" {
                        self.validate_fragment_spread(inner, offset, all_fragments, diagnostics);
                    }
                }
            } else if kind == "field" {
                self.validate_field(
                    child,
                    offset,
                    parent_type,
                    schema,
                    all_fragments,
                    diagnostics,
                    config,
                );
            } else if kind == "fragment_spread" {
                self.validate_fragment_spread(child, offset, all_fragments, diagnostics);
            } else if kind == "inline_fragment" {
                self.validate_inline_fragment(
                    child,
                    offset,
                    parent_type,
                    schema,
                    all_fragments,
                    diagnostics,
                    config,
                );
            }
        }
    }

    pub(super) fn validate_field(
        &self,
        field_node: Node,
        offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        all_fragments: &[String],
        diagnostics: &mut Vec<Diagnostic>,
        config: Option<&crate::Config>,
    ) {
        let mut name_node = None;
        let mut selection_set_node = None;
        let mut arguments_node = None;

        let mut cursor = field_node.walk();
        for child in field_node.children(&mut cursor) {
            if child.kind() == "name" {
                name_node = Some(child);
            } else if child.kind() == "selection_set" {
                selection_set_node = Some(child);
            } else if child.kind() == "arguments" {
                arguments_node = Some(child);
            }
        }

        if let Some(name_node) = name_node {
            let field_name = self.get_node_text(name_node, offset);

            if field_name == "__typename" {
                return;
            }

            let field_def = match parent_type {
                schema::ExtendedType::Object(obj) => obj.fields.get(field_name.as_str()),
                schema::ExtendedType::Interface(iface) => iface.fields.get(field_name.as_str()),
                _ => None,
            };

            if let Some(field_def) = field_def {
                if let Some(directive) = field_def.directives.get("deprecated") {
                    let reason = directive
                        .argument_by_name("reason", schema)
                        .ok()
                        .and_then(|arg| arg.as_str())
                        .unwrap_or("No reason provided");

                    if !self.is_deprecation_ignored(reason, config) {
                        diagnostics.push(Diagnostic {
                            range: self.translate_to_file_range(name_node, offset),
                            severity: Some(DiagnosticSeverity::WARNING),
                            message: format!("Field '{}' is deprecated: {}", field_name, reason),
                            ..Default::default()
                        });
                    }
                }

                if let Some(args_node) = arguments_node {
                    self.validate_arguments(
                        args_node,
                        offset,
                        field_def,
                        schema,
                        diagnostics,
                        config,
                    );
                }

                if let Some(sel_set) = selection_set_node {
                    let field_type_name = field_def.ty.inner_named_type();
                    if let Some(field_type_def) = schema.types.get(field_type_name.as_str()) {
                        self.validate_selection_set(
                            sel_set,
                            offset,
                            field_type_def,
                            schema,
                            all_fragments,
                            diagnostics,
                            config,
                        );
                    }
                }
            } else {
                let type_name = parent_type.name();

                diagnostics.push(Diagnostic {
                    range: self.translate_to_file_range(name_node, offset),
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: format!("Field '{}' not found on type '{}'", field_name, type_name),
                    ..Default::default()
                });
            }
        }
    }
}
