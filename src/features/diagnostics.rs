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
        let query = GQL_DIAGNOSTICS_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_DIAGNOSTICS_QUERY).unwrap()
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
            } else if child.kind() == "variable_definitions" {
                self.validate_variable_definitions(child, offset, schema, diagnostics);
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

    fn validate_variable_definitions(
        &self,
        node: Node,
        offset: usize,
        schema: &Schema,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "variable_definition" {
                let mut vd_cursor = child.walk();
                for vd_child in child.children(&mut vd_cursor) {
                    if vd_child.kind() == "type" {
                        self.validate_type_node(vd_child, offset, schema, diagnostics);
                    }
                }
            }
        }
    }

    fn validate_type_node(
        &self,
        node: Node,
        offset: usize,
        schema: &Schema,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "named_type" => {
                    let type_name = self.get_node_text(child, offset);
                    if let Some(type_def) = schema.types.get(type_name.as_str()) {
                        let directives = match type_def {
                            schema::ExtendedType::Scalar(s) => &s.directives,
                            schema::ExtendedType::Object(o) => &o.directives,
                            schema::ExtendedType::Interface(i) => &i.directives,
                            schema::ExtendedType::Union(u) => &u.directives,
                            schema::ExtendedType::Enum(e) => &e.directives,
                            schema::ExtendedType::InputObject(i) => &i.directives,
                        };

                        if let Some(directive) = directives.get("deprecated") {
                            let reason = directive
                                .argument_by_name("reason", schema)
                                .ok()
                                .and_then(|arg| arg.as_str())
                                .unwrap_or("No reason provided");

                            diagnostics.push(Diagnostic {
                                range: self.translate_to_file_range(child, offset),
                                severity: Some(DiagnosticSeverity::WARNING),
                                message: format!("Type '{}' is deprecated: {}", type_name, reason),
                                ..Default::default()
                            });
                        }
                    }
                }
                "list_type" | "non_null_type" => {
                    self.validate_type_node(child, offset, schema, diagnostics);
                }
                _ => {}
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

                    diagnostics.push(Diagnostic {
                        range: self.translate_to_file_range(name_node, offset),
                        severity: Some(DiagnosticSeverity::WARNING),
                        message: format!("Field '{}' is deprecated: {}", field_name, reason),
                        ..Default::default()
                    });
                }

                if let Some(args_node) = arguments_node {
                    self.validate_arguments(args_node, offset, field_def, schema, diagnostics);
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

    fn validate_arguments(
        &self,
        node: Node,
        offset: usize,
        field_def: &schema::FieldDefinition,
        schema: &Schema,
        diagnostics: &mut Vec<Diagnostic>,
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
                    } else if arg_child.kind().ends_with("_value")
                        || arg_child.kind() == "value"
                        || arg_child.kind() == "variable"
                    {
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
                                .argument_by_name("reason", schema)
                                .ok()
                                .and_then(|arg| arg.as_str())
                                .unwrap_or("No reason provided");

                            diagnostics.push(Diagnostic {
                                range: self.translate_to_file_range(name_node, offset),
                                severity: Some(DiagnosticSeverity::WARNING),
                                message: format!(
                                    "Argument '{}' is deprecated: {}",
                                    arg_name, reason
                                ),
                                ..Default::default()
                            });
                        }

                        if let Some(v_node) = value_node {
                            let arg_type_name = arg_def.ty.inner_named_type();
                            if let Some(arg_type_def) = schema.types.get(arg_type_name.as_str()) {
                                self.validate_value(
                                    v_node,
                                    offset,
                                    arg_type_def,
                                    schema,
                                    diagnostics,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    fn validate_value(
        &self,
        node: Node,
        offset: usize,
        expected_type: &schema::ExtendedType,
        schema: &Schema,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        match node.kind() {
            "value" => {
                if let Some(child) = node.child(0) {
                    self.validate_value(child, offset, expected_type, schema, diagnostics);
                }
            }
            "object_value" => {
                if let schema::ExtendedType::InputObject(input_obj) = expected_type {
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
                                    || field_child.kind() == "variable"
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
                                            .argument_by_name("reason", schema)
                                            .ok()
                                            .and_then(|arg| arg.as_str())
                                            .unwrap_or("No reason provided");

                                        diagnostics.push(Diagnostic {
                                            range: self.translate_to_file_range(name_node, offset),
                                            severity: Some(DiagnosticSeverity::WARNING),
                                            message: format!(
                                                "Input field '{}' is deprecated: {}",
                                                field_name, reason
                                            ),
                                            ..Default::default()
                                        });
                                    }

                                    if let Some(v_node) = value_node {
                                        let field_type_name = field_def.ty.inner_named_type();
                                        if let Some(field_type_def) =
                                            schema.types.get(field_type_name.as_str())
                                        {
                                            self.validate_value(
                                                v_node,
                                                offset,
                                                field_type_def,
                                                schema,
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
            "list_value" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind().ends_with("_value") || child.kind() == "value" {
                        self.validate_value(child, offset, expected_type, schema, diagnostics);
                    }
                }
            }
            "enum_value" => {
                if let schema::ExtendedType::Enum(enum_def) = expected_type {
                    let value_name = self.get_node_text(node, offset);
                    if let Some(value_def) = enum_def.values.get(value_name.as_str()) {
                        if let Some(directive) = value_def.directives.get("deprecated") {
                            let reason = directive
                                .argument_by_name("reason", schema)
                                .ok()
                                .and_then(|arg| arg.as_str())
                                .unwrap_or("No reason provided");

                            diagnostics.push(Diagnostic {
                                range: self.translate_to_file_range(node, offset),
                                severity: Some(DiagnosticSeverity::WARNING),
                                message: format!(
                                    "Enum value '{}' is deprecated: {}",
                                    value_name, reason
                                ),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
            _ => {}
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
