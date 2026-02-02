use crate::document::DocumentState;
use crate::queries::*;
use apollo_compiler::{schema, Schema};
use tower_lsp::lsp_types::*;
use tree_sitter::{Node, QueryCursor, StreamingIterator};

impl DocumentState {
    pub fn get_semantic_diagnostics(
        &self,
        schema: &Schema,
        all_fragments: &[String],
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        let blocks = self.get_graphql_trees();

        for block in blocks {
            let offset = block.offset;
            // 1. Syntax errors
            self.collect_gql_errors(block.tree.root_node(), offset, &mut diagnostics);

            // 2. Schema validation
            self.validate_tree(
                block.tree.root_node(),
                offset,
                schema,
                all_fragments,
                &mut diagnostics,
            );
        }
        diagnostics
    }

    fn validate_tree(
        &self,
        node: Node,
        offset: usize,
        schema: &Schema,
        all_fragments: &[String],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let query = GQL_VALIDATION_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_VALIDATION_QUERY).unwrap()
        });

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, node, |n: Node| {
            let start = n.start_byte();
            let end = n.end_byte();
            self.rope
                .byte_slice((start + offset)..(end + offset))
                .chunks()
        });

        while let Some(m) = matches.next() {
            for cap in m.captures {
                let capture_name = query.capture_names()[cap.index as usize];
                if capture_name == "operation" {
                    self.validate_operation(cap.node, offset, schema, all_fragments, diagnostics);
                } else if capture_name == "fragment" {
                    self.validate_fragment(cap.node, offset, schema, all_fragments, diagnostics);
                }
            }
        }
    }

    fn validate_operation(
        &self,
        node: Node,
        offset: usize,
        schema: &Schema,
        all_fragments: &[String],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let mut operation_type_string = String::from("query");
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if child.kind() == "operation_type" {
                operation_type_string = self.get_node_text(child, offset);
                break;
            }
        }

        let op_type = match operation_type_string.as_str() {
            "query" => Some(apollo_compiler::ast::OperationType::Query),
            "mutation" => Some(apollo_compiler::ast::OperationType::Mutation),
            "subscription" => Some(apollo_compiler::ast::OperationType::Subscription),
            _ => None,
        };

        if let Some(op) = op_type {
            let root_def = schema.root_operation(op);
            if let Some(root_def_name) = root_def {
                if let Some(root_type) = schema.types.get(root_def_name.as_str()) {
                    for child in node.children(&mut cursor) {
                        if child.kind() == "selection_set" {
                            self.validate_selection_set(
                                child,
                                offset,
                                root_type,
                                schema,
                                all_fragments,
                                diagnostics,
                            );
                        }
                    }
                }
            }
        }
    }

    fn validate_fragment(
        &self,
        node: Node,
        offset: usize,
        schema: &Schema,
        all_fragments: &[String],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let mut cursor = node.walk();
        let mut type_condition_node = None;
        let mut selection_set_node = None;

        for child in node.children(&mut cursor) {
            if child.kind() == "type_condition" {
                type_condition_node = Some(child);
            } else if child.kind() == "selection_set" {
                selection_set_node = Some(child);
            }
        }

        if let Some(type_cond) = type_condition_node {
            let mut tc_cursor = type_cond.walk();
            for tc_child in type_cond.children(&mut tc_cursor) {
                if tc_child.kind() == "named_type" {
                    let mut nt_cursor = tc_child.walk();
                    for nt_child in tc_child.children(&mut nt_cursor) {
                        if nt_child.kind() == "name" {
                            let type_name = self.get_node_text(nt_child, offset);
                            if let Some(type_def) = schema.types.get(type_name.as_str()) {
                                if let Some(sel_set) = selection_set_node {
                                    self.validate_selection_set(
                                        sel_set,
                                        offset,
                                        type_def,
                                        schema,
                                        all_fragments,
                                        diagnostics,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn validate_selection_set(
        &self,
        selection_set: Node,
        offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        all_fragments: &[String],
        diagnostics: &mut Vec<Diagnostic>,
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
                        );
                    } else if k == "inline_fragment" {
                        self.validate_inline_fragment(
                            inner,
                            offset,
                            parent_type,
                            schema,
                            all_fragments,
                            diagnostics,
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
                );
            }
        }
    }

    fn validate_field(
        &self,
        field_node: Node,
        offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        all_fragments: &[String],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let mut name_node = None;
        let mut selection_set_node = None;

        let mut cursor = field_node.walk();
        for child in field_node.children(&mut cursor) {
            if child.kind() == "name" {
                name_node = Some(child);
            } else if child.kind() == "selection_set" {
                selection_set_node = Some(child);
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

                    diagnostics.push(Diagnostic {
                        range: self.translate_to_file_range(name_node, offset),
                        severity: Some(DiagnosticSeverity::WARNING),
                        message: format!("Field '{}' is deprecated: {}", field_name, reason),
                        ..Default::default()
                    });
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
                        );
                    }
                }
            } else {
                let type_name = match parent_type {
                    schema::ExtendedType::Object(o) => o.name.as_str(),
                    schema::ExtendedType::Interface(i) => i.name.as_str(),
                    schema::ExtendedType::Union(u) => u.name.as_str(),
                    schema::ExtendedType::Enum(e) => e.name.as_str(),
                    schema::ExtendedType::InputObject(i) => i.name.as_str(),
                    schema::ExtendedType::Scalar(s) => s.name.as_str(),
                };

                diagnostics.push(Diagnostic {
                    range: self.translate_to_file_range(name_node, offset),
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: format!("Field '{}' not found on type '{}'", field_name, type_name),
                    ..Default::default()
                });
            }
        }
    }

    fn validate_inline_fragment(
        &self,
        node: Node,
        offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        all_fragments: &[String],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let mut cursor = node.walk();
        let mut type_condition_node = None;
        let mut selection_set_node = None;

        for child in node.children(&mut cursor) {
            if child.kind() == "type_condition" {
                type_condition_node = Some(child);
            } else if child.kind() == "selection_set" {
                selection_set_node = Some(child);
            }
        }

        let target_type = if let Some(type_cond) = type_condition_node {
            let mut tc_cursor = type_cond.walk();
            let mut found_type = None;
            for tc_child in type_cond.children(&mut tc_cursor) {
                if tc_child.kind() == "named_type" {
                    let mut nt_cursor = tc_child.walk();
                    for nt_child in tc_child.children(&mut nt_cursor) {
                        if nt_child.kind() == "name" {
                            let type_name = self.get_node_text(nt_child, offset);
                            found_type = schema.types.get(type_name.as_str());
                            break;
                        }
                    }
                }
            }
            found_type
        } else {
            Some(parent_type)
        };

        if let Some(t_type) = target_type {
            if let Some(sel_set) = selection_set_node {
                self.validate_selection_set(
                    sel_set,
                    offset,
                    t_type,
                    schema,
                    all_fragments,
                    diagnostics,
                );
            }
        }
    }

    fn validate_fragment_spread(
        &self,
        node: Node,
        offset: usize,
        all_fragments: &[String],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "fragment_name" {
                let mut name_cursor = child.walk();
                for name_child in child.children(&mut name_cursor) {
                    if name_child.kind() == "name" {
                        let name = self.get_node_text(name_child, offset);
                        if !all_fragments.contains(&name) {
                            diagnostics.push(Diagnostic {
                                range: self.translate_to_file_range(name_child, offset),
                                severity: Some(DiagnosticSeverity::ERROR),
                                message: format!("Unknown fragment: {}", name),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
        }
    }

    fn collect_gql_errors(
        &self,
        node: tree_sitter::Node,
        offset_byte: usize,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        if node.is_error() || node.is_missing() {
            let range = self.translate_to_file_range(node, offset_byte);

            diagnostics.push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::ERROR),
                message: format!("GraphQL Syntax Error: unexpected '{}'", node.kind()),
                ..Default::default()
            });
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_gql_errors(child, offset_byte, diagnostics);
        }
    }
}
