use apollo_compiler::{Schema, ast::OperationType, schema};
use graphql_core::document::DocumentState;
use graphql_core::queries::*;
use lsp_types::*;
use tree_sitter::{Node, StreamingIterator};

pub trait DocumentHover {
    fn get_hover_info(&self, position: Position, schema: &Schema) -> Option<Hover>;
    fn get_field_info(
        &self,
        root: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
    ) -> Option<String>;
    fn get_variable_info(
        &self,
        root: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
    ) -> Option<String>;
    fn find_field_in_operation(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
    ) -> Option<String>;
    fn find_field_in_fragment(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
    ) -> Option<String>;
    fn find_field_recursive(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        depth: usize,
    ) -> Option<String>;
    fn find_field_info(
        &self,
        field_node: Node,
        offset: usize,
        cursor_offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        depth: usize,
    ) -> Option<String>;
    fn get_builtin_field_info(
        &self,
        field_name: &str,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
    ) -> Option<String>;
    fn describe_typename(&self, parent_type: &schema::ExtendedType, schema: &Schema) -> String;
    fn find_field_in_inline_fragment(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        depth: usize,
    ) -> Option<String>;
    fn find_argument_info(
        &self,
        arguments_node: Node,
        offset: usize,
        cursor_offset: usize,
        arg_defs: &[apollo_compiler::Node<schema::InputValueDefinition>],
        schema: &Schema,
    ) -> Option<String>;
    fn find_directive_info_on_node(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
    ) -> Option<String>;
    fn find_directive_info(
        &self,
        directives_node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
    ) -> Option<String>;
    fn find_single_directive_info(
        &self,
        directive_node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
    ) -> Option<String>;
    fn find_info_in_value(
        &self,
        value_node: Node,
        offset: usize,
        cursor_offset: usize,
        ty: &apollo_compiler::ast::Type,
        schema: &Schema,
    ) -> Option<String>;
    fn find_info_in_object_value(
        &self,
        object_value_node: Node,
        offset: usize,
        cursor_offset: usize,
        ty: &apollo_compiler::ast::Type,
        schema: &Schema,
    ) -> Option<String>;
    fn get_type_info_from_schema(&self, name: &str, schema: &Schema) -> Option<String>;
    fn find_description(&self, target_name: &str) -> Option<String>;
    fn get_alias_name(&self, alias_node: Node, offset: usize) -> String;
    fn find_type_extension_info(&self, name_node: Node, offset: usize) -> Option<String>;
    fn extract_operation_variables(&self, op_node: Node, offset: usize) -> Vec<(String, String)>;
    fn extract_field_info_for_alias(
        &self,
        field_node: Node,
        offset: usize,
        cursor_offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
    ) -> Option<(String, String, Option<String>)>;
}

impl DocumentHover for DocumentState {
    fn get_hover_info(&self, position: Position, schema: &Schema) -> Option<Hover> {
        let byte_offset = self.position_to_byte(position);

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();
            let tree_len = root.end_byte();

            if byte_offset >= offset && byte_offset < offset + tree_len {
                let local_byte = byte_offset - offset;
                let mut node = root.descendant_for_byte_range(local_byte, local_byte)?;

                // If we are on a symbol that's part of a larger construct, move up
                if node.kind() == "$"
                    && let Some(parent) = node.parent()
                {
                    node = parent;
                }

                if node.kind() == "name"
                    || node.kind() == "variable"
                    || node.kind() == "string_value"
                    || node.kind() == "int_value"
                    || node.kind() == "float_value"
                    || node.kind() == "boolean_value"
                    || node.kind() == "null_value"
                {
                    let symbol_name = self
                        .rope
                        .slice(
                            self.rope.byte_to_char(node.start_byte() + offset)
                                ..self.rope.byte_to_char(node.end_byte() + offset),
                        )
                        .to_string();

                    if (node.kind() == "variable"
                        || node.parent().is_some_and(|p| p.kind() == "variable"))
                        && let Some(var_info) =
                            self.get_variable_info(root, offset, byte_offset, schema)
                    {
                        return Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: var_info,
                            }),
                            range: Some(self.translate_to_file_range(node, offset)),
                        });
                    }

                    if let Some(extension_info) = self.find_type_extension_info(node, offset) {
                        return Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: extension_info,
                            }),
                            range: Some(self.translate_to_file_range(node, offset)),
                        });
                    }

                    if let Some(schema_info) = self.get_type_info_from_schema(&symbol_name, schema)
                    {
                        return Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: schema_info,
                            }),
                            range: Some(self.translate_to_file_range(node, offset)),
                        });
                    }

                    if let Some(description) = self.find_description(&symbol_name) {
                        return Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: format!("### {}\n---\n{}", symbol_name, description),
                            }),
                            range: Some(self.translate_to_file_range(node, offset)),
                        });
                    }

                    if let Some(field_info) = self.get_field_info(root, offset, byte_offset, schema)
                    {
                        return Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: field_info,
                            }),
                            range: Some(self.translate_to_file_range(node, offset)),
                        });
                    }
                }

                // Check for variable default value
                let mut curr = node;
                while let Some(parent) = curr.parent() {
                    if parent.kind() == "variable_definition" {
                        let mut vd_cursor = parent.walk();
                        let mut var_type = None;
                        for vd_child in parent.children(&mut vd_cursor) {
                            if vd_child.kind() == "type" {
                                var_type = Some(self.get_node_text(vd_child, offset));
                            } else if vd_child.kind() == "default_value" {
                                let range = (vd_child.start_byte() + offset)
                                    ..(vd_child.end_byte() + offset);
                                if byte_offset >= range.start
                                    && byte_offset <= range.end
                                    && let Some(ty_text) = var_type
                                {
                                    return Some(Hover {
                                        contents: HoverContents::Markup(MarkupContent {
                                            kind: MarkupKind::Markdown,
                                            value: format!(
                                                "### default value\n---\nType: `{}`\n\nMatches variable type",
                                                ty_text
                                            ),
                                        }),
                                        range: Some(self.translate_to_file_range(vd_child, offset)),
                                    });
                                }
                            }
                        }
                    }
                    curr = parent;
                }
            }
        }
        None
    }

    fn get_field_info(
        &self,
        root: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
    ) -> Option<String> {
        let query = GQL_DIAGNOSTICS_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_DIAGNOSTICS_QUERY).unwrap()
        });

        let mut cursor = tree_sitter::QueryCursor::new();
        let mut matches = cursor.matches(query, root, |n: Node| {
            let start = n.start_byte();
            let end = n.end_byte();
            self.rope
                .byte_slice((start + offset)..(end + offset))
                .chunks()
        });

        while let Some(m) = matches.next() {
            for cap in m.captures {
                let range = (cap.node.start_byte() + offset)..(cap.node.end_byte() + offset);

                if cursor_offset >= range.start && cursor_offset <= range.end {
                    let capture_name = query.capture_names()[cap.index as usize];
                    match capture_name {
                        "operation" => {
                            return self.find_field_in_operation(
                                cap.node,
                                offset,
                                cursor_offset,
                                schema,
                            );
                        }
                        "fragment" => {
                            return self.find_field_in_fragment(
                                cap.node,
                                offset,
                                cursor_offset,
                                schema,
                            );
                        }
                        _ => {}
                    }
                }
            }
        }
        None
    }

    fn get_variable_info(
        &self,
        root: Node,
        offset: usize,
        cursor_offset: usize,
        _schema: &Schema,
    ) -> Option<String> {
        let node =
            root.descendant_for_byte_range(cursor_offset - offset, cursor_offset - offset)?;
        let mut var_node = if node.kind() == "variable" {
            node
        } else if node.parent()?.kind() == "variable" {
            node.parent()?
        } else {
            return None;
        };

        if var_node.kind() == "$"
            && let Some(p) = var_node.parent()
            && p.kind() == "variable"
        {
            var_node = p;
        }

        let var_name = self.get_node_text(var_node, offset);

        // Find the operation or fragment containing this variable
        let mut curr = var_node;
        while let Some(parent) = curr.parent() {
            if parent.kind() == "operation_definition" {
                // Look for variable definitions in this operation
                let mut cursor = parent.walk();
                for child in parent.children(&mut cursor) {
                    if child.kind() == "variable_definitions" {
                        let mut vd_cursor = child.walk();
                        for vd in child.children(&mut vd_cursor) {
                            if vd.kind() == "variable_definition" {
                                let mut v = None;
                                let mut ty = None;
                                let mut v_cursor = vd.walk();
                                for v_child in vd.children(&mut v_cursor) {
                                    if v_child.kind() == "variable" {
                                        v = Some(v_child);
                                    } else if v_child.kind() == "type" {
                                        ty = Some(v_child);
                                    }
                                }

                                if let Some(v) = v
                                    && self.get_node_text(v, offset) == var_name
                                    && let Some(ty_node) = ty
                                {
                                    let ty_text = self.get_node_text(ty_node, offset);
                                    return Some(describe_variable_markdown(&var_name, &ty_text));
                                }
                            }
                        }
                    }
                }
            }
            curr = parent;
        }

        None
    }

    fn find_field_in_operation(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
    ) -> Option<String> {
        if let Some(info) = self.find_directive_info_on_node(node, offset, cursor_offset, schema) {
            return Some(info);
        }

        let operation_type_string = self.get_operation_type(node, offset);

        if let Some(name_node) = self.find_child_by_kind(node, "name") {
            let name_range = (name_node.start_byte() + offset)..(name_node.end_byte() + offset);
            if cursor_offset >= name_range.start && cursor_offset <= name_range.end {
                let op_name = self.get_node_text(name_node, offset);
                let variables = self.extract_operation_variables(node, offset);
                let description = self.find_description(&op_name);
                return Some(describe_operation_markdown(
                    &operation_type_string,
                    Some(&op_name),
                    &variables,
                    description.as_deref(),
                ));
            }
        }

        let op_type = match operation_type_string.as_str() {
            "query" => Some(OperationType::Query),
            "mutation" => Some(OperationType::Mutation),
            "subscription" => Some(OperationType::Subscription),
            _ => None,
        };

        if let Some(op) = op_type
            && let Some(root_def_name) = schema.root_operation(op)
            && let Some(root_type) = schema.types.get(root_def_name.as_str())
        {
            return self.find_field_recursive(node, offset, cursor_offset, root_type, schema, 0);
        }
        None
    }

    fn find_field_in_fragment(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
    ) -> Option<String> {
        if let Some(info) = self.find_directive_info_on_node(node, offset, cursor_offset, schema) {
            return Some(info);
        }

        if let Some(child) = self.find_child_by_kind(node, "selection_set")
            && self.is_cursor_in_node_range(child, offset, cursor_offset)
            && let Some(type_name) = self.get_fragment_type_condition(node, offset)
            && let Some(type_def) = schema.types.get(type_name.as_str())
        {
            return self.find_field_recursive(child, offset, cursor_offset, type_def, schema, 0);
        }
        None
    }

    fn find_field_recursive(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        depth: usize,
    ) -> Option<String> {
        if depth > 100 {
            return None;
        }
        let target_node = if node.kind() == "selection_set" {
            node
        } else {
            self.find_child_by_kind(node, "selection_set")?
        };

        let mut cursor = target_node.walk();
        for child in target_node.children(&mut cursor) {
            if self.is_cursor_in_node_range(child, offset, cursor_offset) {
                let kind = child.kind();
                if kind == "selection" {
                    let mut inner = child.walk();
                    for inner_child in child.children(&mut inner) {
                        if inner_child.kind() == "field" {
                            if let Some(info) = self.find_field_info(
                                inner_child,
                                offset,
                                cursor_offset,
                                parent_type,
                                schema,
                                depth + 1,
                            ) {
                                return Some(info);
                            }
                        } else if inner_child.kind() == "inline_fragment"
                            && let Some(info) = self.find_field_in_inline_fragment(
                                inner_child,
                                offset,
                                cursor_offset,
                                parent_type,
                                schema,
                                depth + 1,
                            )
                        {
                            return Some(info);
                        }
                    }
                } else if kind == "field" {
                    if let Some(info) = self.find_field_info(
                        child,
                        offset,
                        cursor_offset,
                        parent_type,
                        schema,
                        depth + 1,
                    ) {
                        return Some(info);
                    }
                } else if kind == "inline_fragment"
                    && let Some(info) = self.find_field_in_inline_fragment(
                        child,
                        offset,
                        cursor_offset,
                        parent_type,
                        schema,
                        depth + 1,
                    )
                {
                    return Some(info);
                }
            }
        }
        None
    }

    fn find_field_info(
        &self,
        field_node: Node,
        offset: usize,
        cursor_offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        depth: usize,
    ) -> Option<String> {
        if depth > 100 {
            return None;
        }

        if let Some(info) =
            self.find_directive_info_on_node(field_node, offset, cursor_offset, schema)
        {
            return Some(info);
        }

        let components = self.extract_field_components(field_node);

        if let Some(alias_node) = components.alias {
            let alias_range = (alias_node.start_byte() + offset)..(alias_node.end_byte() + offset);
            if cursor_offset >= alias_range.start && cursor_offset <= alias_range.end {
                let alias_name = self.get_alias_name(alias_node, offset);
                let field_info = self.extract_field_info_for_alias(
                    field_node,
                    offset,
                    cursor_offset,
                    parent_type,
                    schema,
                );
                if let Some((field_name, field_type, field_desc)) = field_info {
                    let hover_text = format!(
                        "### alias `{}` → field `{}.{}`\n---\nType: `{}`\n",
                        alias_name,
                        parent_type.name(),
                        field_name,
                        field_type
                    );
                    if let Some(desc) = field_desc
                        && !desc.trim().is_empty()
                    {
                        return Some(format!("{}{}", hover_text, desc));
                    }
                    return Some(hover_text);
                }
            }
        }

        if let Some(name_node) = components.name {
            let field_name = self.get_node_text(name_node, offset);
            let name_range = (name_node.start_byte() + offset)..(name_node.end_byte() + offset);

            let field_def = match parent_type {
                schema::ExtendedType::Object(obj) => obj.fields.get(field_name.as_str()),
                schema::ExtendedType::Interface(iface) => iface.fields.get(field_name.as_str()),
                _ => None,
            };

            if let Some(field_def) = field_def {
                if cursor_offset >= name_range.start && cursor_offset <= name_range.end {
                    let alias_name = components
                        .alias
                        .as_ref()
                        .map(|a| self.get_alias_name(*a, offset));
                    if let Some(alias) = alias_name {
                        let hover_text = format!(
                            "### field `{}.{}` (aliased as `{}`)\n---\nType: `{}`\n",
                            parent_type.name(),
                            field_name,
                            alias,
                            field_def.ty
                        );
                        if let Some(desc) = field_def.description.as_deref()
                            && !desc.trim().is_empty()
                        {
                            return Some(format!("{}{}", hover_text, desc));
                        }
                        return Some(hover_text);
                    }
                    return Some(describe_field_markdown(
                        parent_type.name(),
                        field_name.as_str(),
                        field_def.ty.to_string().as_str(),
                        field_def.description.as_deref(),
                    ));
                }

                if let Some(args_node) = components.arguments
                    && self.is_cursor_in_node_range(args_node, offset, cursor_offset)
                    && let Some(info) = self.find_argument_info(
                        args_node,
                        offset,
                        cursor_offset,
                        &field_def.arguments,
                        schema,
                    )
                {
                    return Some(info);
                }

                if let Some(info) =
                    self.find_directive_info_on_node(field_node, offset, cursor_offset, schema)
                {
                    return Some(info);
                }

                if let Some(sss) = components.selection_set
                    && self.is_cursor_in_node_range(sss, offset, cursor_offset)
                {
                    let field_type_name = field_def.ty.inner_named_type();
                    if let Some(field_type_def) = schema.types.get(field_type_name.as_str()) {
                        return self.find_field_recursive(
                            sss,
                            offset,
                            cursor_offset,
                            field_type_def,
                            schema,
                            depth + 1,
                        );
                    }
                }
            } else if cursor_offset >= name_range.start
                && cursor_offset <= name_range.end
                && let Some(info) = self.get_builtin_field_info(&field_name, parent_type, schema)
            {
                return Some(info);
            }
        }
        None
    }

    fn get_builtin_field_info(
        &self,
        field_name: &str,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
    ) -> Option<String> {
        match field_name {
            "__typename" => Some(self.describe_typename(parent_type, schema)),
            "__schema" | "__type" => {
                if !is_schema_query_root(parent_type, schema) {
                    return None;
                }

                let fallback_desc = if field_name == "__schema" {
                    "Access the current schema introspection object."
                } else {
                    "Look up a type definition by its name."
                };

                let fallback_type = if field_name == "__schema" {
                    "`__Schema!`"
                } else {
                    "`__Type`"
                };

                let title = format!("### field {}.{}\n---\n", parent_type.name(), field_name);
                let mut info = format!("{title}Type: {fallback_type}\n");

                if let Some((schema_type, description)) =
                    schema_field_strings(parent_type, field_name, schema)
                {
                    return Some(describe_field_markdown(
                        parent_type.name(),
                        field_name,
                        schema_type.as_str(),
                        description.as_deref(),
                    ));
                }

                info.push('\n');
                info.push_str(fallback_desc);
                Some(info)
            }
            _ => None,
        }
    }

    fn describe_typename(&self, parent_type: &schema::ExtendedType, schema: &Schema) -> String {
        if let Some((field_type, description)) =
            schema_field_strings(parent_type, "__typename", schema)
        {
            return describe_field_markdown(
                parent_type.name(),
                "__typename",
                field_type.as_str(),
                description.as_deref(),
            );
        }

        format!(
            "### field {}.__typename\n---\nType: `String!`\n\nThe GraphQL type name of the current selection.",
            parent_type.name()
        )
    }

    fn find_field_in_inline_fragment(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        depth: usize,
    ) -> Option<String> {
        if depth > 100 {
            return None;
        }

        if let Some(info) = self.find_directive_info_on_node(node, offset, cursor_offset, schema) {
            return Some(info);
        }

        let mut target_type = parent_type;
        let mut selection_set_node = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "type_condition" => {
                    if let Some(type_name) = self.get_fragment_type_condition(node, offset)
                        && let Some(new_type) = schema.types.get(type_name.as_str())
                    {
                        target_type = new_type;
                    }
                }
                "selection_set" => {
                    selection_set_node = Some(child);
                }
                _ => {}
            }
        }

        if let Some(ss) = selection_set_node {
            let ss_range = (ss.start_byte() + offset)..(ss.end_byte() + offset);
            if cursor_offset >= ss_range.start && cursor_offset <= ss_range.end {
                return self.find_field_recursive(
                    ss,
                    offset,
                    cursor_offset,
                    target_type,
                    schema,
                    depth + 1,
                );
            }
        }
        None
    }

    fn find_argument_info(
        &self,
        arguments_node: Node,
        offset: usize,
        cursor_offset: usize,
        arg_defs: &[apollo_compiler::Node<schema::InputValueDefinition>],
        schema: &Schema,
    ) -> Option<String> {
        let mut cursor = arguments_node.walk();
        for argument_node in arguments_node.children(&mut cursor) {
            if argument_node.kind() == "argument" {
                let arg_range =
                    (argument_node.start_byte() + offset)..(argument_node.end_byte() + offset);
                if cursor_offset >= arg_range.start && cursor_offset <= arg_range.end {
                    let name_node = self.find_child_by_kind(argument_node, "name");
                    let value_node = self.find_child_by_kind(argument_node, "value");

                    if let Some(name_node) = name_node {
                        let name_range =
                            (name_node.start_byte() + offset)..(name_node.end_byte() + offset);
                        if cursor_offset >= name_range.start && cursor_offset <= name_range.end {
                            let arg_name = self.get_node_text(name_node, offset);
                            if let Some(arg_def) =
                                arg_defs.iter().find(|a| a.name.as_str() == arg_name)
                            {
                                return Some(describe_argument_markdown(
                                    &arg_name,
                                    &arg_def.ty.to_string(),
                                    arg_def.description.as_deref(),
                                ));
                            }
                        }
                    }

                    if let Some(value_node) = value_node {
                        let value_range =
                            (value_node.start_byte() + offset)..(value_node.end_byte() + offset);
                        if cursor_offset >= value_range.start && cursor_offset <= value_range.end {
                            let arg_name = name_node
                                .map(|n| self.get_node_text(n, offset))
                                .unwrap_or_default();
                            if let Some(arg_def) =
                                arg_defs.iter().find(|a| a.name.as_str() == arg_name)
                            {
                                return self.find_info_in_value(
                                    value_node,
                                    offset,
                                    cursor_offset,
                                    &arg_def.ty,
                                    schema,
                                );
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn find_directive_info_on_node(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
    ) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "directives" => {
                    let range = (child.start_byte() + offset)..(child.end_byte() + offset);
                    if cursor_offset >= range.start && cursor_offset <= range.end {
                        return self.find_directive_info(child, offset, cursor_offset, schema);
                    }
                }
                "directive" => {
                    let range = (child.start_byte() + offset)..(child.end_byte() + offset);
                    if cursor_offset >= range.start && cursor_offset <= range.end {
                        return self.find_single_directive_info(
                            child,
                            offset,
                            cursor_offset,
                            schema,
                        );
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn find_directive_info(
        &self,
        directives_node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
    ) -> Option<String> {
        let mut cursor = directives_node.walk();
        for directive_node in directives_node.children(&mut cursor) {
            if directive_node.kind() == "directive"
                && let Some(info) =
                    self.find_single_directive_info(directive_node, offset, cursor_offset, schema)
            {
                return Some(info);
            }
        }
        None
    }

    fn find_single_directive_info(
        &self,
        directive_node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
    ) -> Option<String> {
        let dir_range =
            (directive_node.start_byte() + offset)..(directive_node.end_byte() + offset);
        if cursor_offset >= dir_range.start && cursor_offset <= dir_range.end {
            let name_node = self.find_child_by_kind(directive_node, "name");
            let args_node = self.find_child_by_kind(directive_node, "arguments");

            if let Some(name_node) = name_node {
                let name_range = (name_node.start_byte() + offset)..(name_node.end_byte() + offset);
                let dir_name = self.get_node_text(name_node, offset);
                if let Some(dir_def) = schema.directive_definitions.get(dir_name.as_str()) {
                    if cursor_offset >= name_range.start && cursor_offset <= name_range.end {
                        return Some(describe_directive_markdown(
                            &dir_name,
                            dir_def.description.as_deref(),
                            &dir_def.arguments,
                        ));
                    }

                    if let Some(args_node) = args_node {
                        let args_range =
                            (args_node.start_byte() + offset)..(args_node.end_byte() + offset);
                        if cursor_offset >= args_range.start && cursor_offset <= args_range.end {
                            return self.find_argument_info(
                                args_node,
                                offset,
                                cursor_offset,
                                &dir_def.arguments,
                                schema,
                            );
                        }
                    }
                }
            }
        }
        None
    }

    fn find_info_in_value(
        &self,
        value_node: Node,
        offset: usize,
        cursor_offset: usize,
        ty: &apollo_compiler::ast::Type,
        schema: &Schema,
    ) -> Option<String> {
        if value_node.kind() == "enum_value" {
            let type_name = ty.inner_named_type();
            if let Some(schema::ExtendedType::Enum(enm)) = schema.types.get(type_name.as_str()) {
                let value_name = self.get_node_text(value_node, offset);
                if let Some(val_def) = enm.values.get(value_name.as_str()) {
                    let deprecation_reason = val_def.directives.get("deprecated").and_then(|d| {
                        d.argument_by_name("reason", schema)
                            .ok()
                            .and_then(|arg| arg.as_str())
                    });
                    return Some(describe_enum_value_markdown(
                        type_name.as_str(),
                        value_name.as_str(),
                        val_def.description.as_deref(),
                        deprecation_reason,
                    ));
                }
            }
        }

        let mut cursor = value_node.walk();
        for child in value_node.children(&mut cursor) {
            match child.kind() {
                "string_value" | "int_value" | "float_value" | "boolean_value" | "null_value" => {
                    return Some(describe_literal_markdown(child.kind(), &ty.to_string()));
                }
                "object_value" => {
                    return self.find_info_in_object_value(
                        child,
                        offset,
                        cursor_offset,
                        ty,
                        schema,
                    );
                }
                "list_value" => {
                    return self.find_info_in_value(child, offset, cursor_offset, ty, schema);
                }
                "enum_value" => {
                    let type_name = ty.inner_named_type();
                    if let Some(schema::ExtendedType::Enum(enm)) =
                        schema.types.get(type_name.as_str())
                    {
                        let value_name = self.get_node_text(child, offset);
                        if let Some(val_def) = enm.values.get(value_name.as_str()) {
                            let deprecation_reason =
                                val_def.directives.get("deprecated").and_then(|d| {
                                    d.argument_by_name("reason", schema)
                                        .ok()
                                        .and_then(|arg| arg.as_str())
                                });
                            return Some(describe_enum_value_markdown(
                                type_name.as_str(),
                                value_name.as_str(),
                                val_def.description.as_deref(),
                                deprecation_reason,
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn find_info_in_object_value(
        &self,
        object_value_node: Node,
        offset: usize,
        cursor_offset: usize,
        ty: &apollo_compiler::ast::Type,
        schema: &Schema,
    ) -> Option<String> {
        let type_name = ty.inner_named_type();
        let type_def = schema.types.get(type_name.as_str())?;
        let input_obj = match type_def {
            schema::ExtendedType::InputObject(io) => io,
            _ => return None,
        };

        let mut cursor = object_value_node.walk();
        for field_node in object_value_node.children(&mut cursor) {
            if field_node.kind() == "object_field" {
                let range = (field_node.start_byte() + offset)..(field_node.end_byte() + offset);
                if cursor_offset >= range.start && cursor_offset <= range.end {
                    let name_node = self.find_child_by_kind(field_node, "name");
                    let val_node = self.find_child_by_kind(field_node, "value");

                    if let Some(name_node) = name_node {
                        let name_range =
                            (name_node.start_byte() + offset)..(name_node.end_byte() + offset);
                        if cursor_offset >= name_range.start && cursor_offset <= name_range.end {
                            let field_name = self.get_node_text(name_node, offset);
                            if let Some(field_def) = input_obj.fields.get(field_name.as_str()) {
                                return Some(describe_field_markdown(
                                    type_name.as_str(),
                                    field_name.as_str(),
                                    field_def.ty.to_string().as_str(),
                                    field_def.description.as_deref(),
                                ));
                            }
                        }
                    }

                    if let Some(val_node) = val_node {
                        let field_name = name_node
                            .map(|n| self.get_node_text(n, offset))
                            .unwrap_or_default();
                        if let Some(field_def) = input_obj.fields.get(field_name.as_str()) {
                            return self.find_info_in_value(
                                val_node,
                                offset,
                                cursor_offset,
                                &field_def.ty,
                                schema,
                            );
                        }
                    }
                }
            }
        }
        None
    }

    fn get_type_info_from_schema(&self, name: &str, schema: &Schema) -> Option<String> {
        let ty = schema.types.get(name)?;

        let mut output = String::new();

        match ty {
            schema::ExtendedType::Scalar(_) => output.push_str(&format!("### scalar {}\n", name)),
            schema::ExtendedType::Object(_) => output.push_str(&format!("### type {}\n", name)),
            schema::ExtendedType::Interface(_) => {
                output.push_str(&format!("### interface {}\n", name))
            }
            schema::ExtendedType::Union(_) => output.push_str(&format!("### union {}\n", name)),
            schema::ExtendedType::Enum(_) => output.push_str(&format!("### enum {}\n", name)),
            schema::ExtendedType::InputObject(_) => {
                output.push_str(&format!("### input {}\n", name))
            }
        }

        output.push_str("---\n");

        if let Some(desc) = ty.description() {
            output.push_str(desc);
            output.push_str("\n\n");
        }

        match ty {
            schema::ExtendedType::Object(obj) => {
                output.push_str("#### Fields\n");
                for (field_name, field_def) in &obj.fields {
                    output.push_str(&format!("- **{}**: `{}`\n", field_name, field_def.ty));
                }
            }
            schema::ExtendedType::Interface(iface) => {
                output.push_str("#### Fields\n");
                for (field_name, field_def) in &iface.fields {
                    output.push_str(&format!("- **{}**: `{}`\n", field_name, field_def.ty));
                }
            }
            schema::ExtendedType::InputObject(input) => {
                output.push_str("#### Fields\n");
                for (field_name, field_def) in &input.fields {
                    output.push_str(&format!("- **{}**: `{}`\n", field_name, field_def.ty));
                }
            }
            schema::ExtendedType::Enum(enm) => {
                output.push_str("#### Values\n");
                for (val_name, _) in &enm.values {
                    output.push_str(&format!("- `{}`\n", val_name));
                }
            }
            _ => {}
        }

        Some(output)
    }

    fn find_description(&self, target_name: &str) -> Option<String> {
        let query = GQL_DESCRIPTION_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_DESCRIPTION_QUERY).unwrap()
        });

        let mut cursor = tree_sitter::QueryCursor::new();

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let mut matches =
                cursor.matches(query, block.tree.root_node(), |node: tree_sitter::Node| {
                    let start = node.start_byte();
                    let end = node.end_byte();
                    self.rope
                        .byte_slice((start + offset)..(end + offset))
                        .chunks()
                });

            while let Some(m) = matches.next() {
                let node = m.captures[0].node;
                if node.kind() == "comment" {
                    continue;
                }

                let container = node;
                let mut name = None;
                let mut description = None;

                let mut cursor = container.walk();
                for child in container.children(&mut cursor) {
                    match child.kind() {
                        "name" => {
                            name = Some(self.get_node_text(child, offset));
                        }
                        "enum_value" => {
                            if let Some(n) = child.child_by_field_name("name") {
                                name = Some(self.get_node_text(n, offset));
                            } else if let Some(n) = child.child(0) {
                                name = Some(self.get_node_text(n, offset));
                            }
                        }
                        "fragment_name" => {
                            if let Some(n) = child.child_by_field_name("name") {
                                name = Some(self.get_node_text(n, offset));
                            } else if let Some(n) = child.child(0) {
                                // Fallback for some grammar versions
                                name = Some(self.get_node_text(n, offset));
                            }
                        }
                        "description" => {
                            if let Some(sv) = child.child_by_field_name("content") {
                                description = Some(self.get_node_text(sv, offset));
                            } else if let Some(sv) = child.child(0) {
                                description = Some(self.get_node_text(sv, offset));
                            }
                        }
                        "string_value" => {
                            description = Some(self.get_node_text(child, offset));
                        }
                        _ => {}
                    }
                }

                if description.is_none() {
                    // Try to find preceding comment by looking at the line above
                    let range = self.translate_to_file_range(container, offset);
                    if range.start.line > 0 {
                        let prev_line_num = range.start.line - 1;
                        let line_start = self.rope.line_to_char(prev_line_num as usize);
                        let line_end = self.rope.line_to_char(range.start.line as usize);
                        let line_text = self.rope.slice(line_start..line_end).to_string();
                        let trimmed = line_text.trim();
                        if trimmed.starts_with('#') {
                            description = Some(trimmed.trim_start_matches('#').trim().to_string());
                        }
                    }
                }

                if let Some(n) = name
                    && n == target_name
                {
                    return description.map(|d| d.trim_matches('"').to_string());
                }
            }
        }
        None
    }

    fn get_alias_name(&self, alias_node: Node, offset: usize) -> String {
        let mut cursor = alias_node.walk();
        for child in alias_node.children(&mut cursor) {
            if child.kind() == "name" {
                return self.get_node_text(child, offset);
            }
        }
        self.get_node_text(alias_node, offset)
    }

    fn find_type_extension_info(&self, name_node: Node, offset: usize) -> Option<String> {
        let mut curr = name_node;
        while let Some(parent) = curr.parent() {
            match parent.kind() {
                "object_type_extension"
                | "interface_type_extension"
                | "enum_type_extension"
                | "scalar_type_extension"
                | "union_type_extension"
                | "input_object_type_extension" => {
                    let type_name = self.get_node_text(name_node, offset);
                    let mut adds_fields = Vec::new();
                    let mut implements_interfaces = Vec::new();

                    let mut cursor = parent.walk();
                    for child in parent.children(&mut cursor) {
                        match child.kind() {
                            "implements_interfaces" => {
                                let mut i_cursor = child.walk();
                                for i_child in child.children(&mut i_cursor) {
                                    if i_child.kind() == "named_type" {
                                        implements_interfaces
                                            .push(self.get_node_text(i_child, offset));
                                    }
                                }
                            }
                            "field_definitions" | "fields_definition" => {
                                let mut fd_cursor = child.walk();
                                for fd in child.children(&mut fd_cursor) {
                                    if fd.kind() == "field_definition" {
                                        let f_name = self
                                            .find_child_by_kind(fd, "name")
                                            .map(|n| self.get_node_text(n, offset))
                                            .unwrap_or_default();
                                        let f_type = self
                                            .find_child_by_kind(fd, "type")
                                            .map(|n| self.get_node_text(n, offset))
                                            .unwrap_or_default();
                                        if !f_name.is_empty() {
                                            adds_fields.push(format!("{}: {}", f_name, f_type));
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }

                    let mut info = format!("### extends {}\n---\n", type_name);
                    if !adds_fields.is_empty() {
                        info.push_str("Adds: ");
                        let fields: Vec<String> =
                            adds_fields.iter().map(|f| format!("`{}`", f)).collect();
                        info.push_str(&fields.join(", "));
                        info.push('\n');
                    }
                    if !implements_interfaces.is_empty() {
                        info.push_str("Implements: ");
                        let ifaces: Vec<String> = implements_interfaces
                            .iter()
                            .map(|i| format!("`{}`", i))
                            .collect();
                        info.push_str(&ifaces.join(", "));
                        info.push('\n');
                    }
                    return Some(info);
                }
                _ => {}
            }
            curr = parent;
        }
        None
    }

    fn extract_operation_variables(&self, op_node: Node, offset: usize) -> Vec<(String, String)> {
        let mut variables = Vec::new();
        if let Some(defs) = self.find_child_by_kind(op_node, "variable_definitions") {
            let mut cursor = defs.walk();
            for vd in defs.children(&mut cursor) {
                if vd.kind() == "variable_definition" {
                    let mut v_name = String::new();
                    let mut v_type = String::new();
                    let mut inner = vd.walk();
                    for child in vd.children(&mut inner) {
                        if child.kind() == "variable" {
                            v_name = self.get_node_text(child, offset);
                        } else if child.kind() == "type" {
                            v_type = self.get_node_text(child, offset);
                        }
                    }
                    if !v_name.is_empty() {
                        variables.push((v_name, v_type));
                    }
                }
            }
        }
        variables
    }

    fn extract_field_info_for_alias(
        &self,
        field_node: Node,
        offset: usize,
        _cursor_offset: usize,
        parent_type: &schema::ExtendedType,
        _schema: &Schema,
    ) -> Option<(String, String, Option<String>)> {
        let components = self.extract_field_components(field_node);
        if let Some(name_node) = components.name {
            let field_name = self.get_node_text(name_node, offset);
            let field_def = match parent_type {
                schema::ExtendedType::Object(obj) => obj.fields.get(field_name.as_str()),
                schema::ExtendedType::Interface(iface) => iface.fields.get(field_name.as_str()),
                _ => None,
            };

            if let Some(fd) = field_def {
                let ty = fd.ty.to_string();
                let desc = fd.description.as_ref().map(|d| d.to_string());
                return Some((field_name, ty, desc));
            }
        }
        None
    }
}

// Helper functions

fn is_schema_query_root(ty: &schema::ExtendedType, schema: &Schema) -> bool {
    schema
        .root_operation(OperationType::Query)
        .and_then(|root_name| schema.types.get(root_name.as_str()))
        .map(|root_type| root_type.name() == ty.name())
        .unwrap_or(false)
}

fn schema_field_strings(
    parent_type: &schema::ExtendedType,
    field_name: &str,
    schema: &Schema,
) -> Option<(String, Option<String>)> {
    let candidate = match parent_type {
        schema::ExtendedType::Object(obj) => obj.fields.get(field_name),
        schema::ExtendedType::Interface(iface) => iface.fields.get(field_name),
        _ => schema
            .types
            .get(parent_type.name().as_str())
            .and_then(|ty| match ty {
                schema::ExtendedType::Object(obj) => obj.fields.get(field_name),
                schema::ExtendedType::Interface(iface) => iface.fields.get(field_name),
                _ => None,
            }),
    }?;

    let ty = candidate.ty.to_string();
    let description = candidate
        .description
        .as_ref()
        .map(|d| d.as_ref().to_string());

    Some((ty, description))
}

fn describe_field_markdown(
    parent_name: &str,
    field_name: &str,
    field_type: &str,
    description: Option<&str>,
) -> String {
    let mut info = format!(
        "### field {}.{}\n---\nType: `{}`\n",
        parent_name, field_name, field_type
    );
    if let Some(desc) = description
        && !desc.trim().is_empty()
    {
        info.push('\n');
        info.push_str(desc);
    }
    info
}

fn describe_argument_markdown(arg_name: &str, arg_type: &str, description: Option<&str>) -> String {
    let mut info = format!("### argument {}\n---\nType: `{}`\n", arg_name, arg_type);
    if let Some(desc) = description
        && !desc.trim().is_empty()
    {
        info.push('\n');
        info.push_str(desc);
    }
    info
}

fn describe_directive_markdown(
    dir_name: &str,
    description: Option<&str>,
    arguments: &[apollo_compiler::Node<schema::InputValueDefinition>],
) -> String {
    let mut info = format!("### directive @{}\n---\n", dir_name);
    if !arguments.is_empty() {
        info.push_str("Args: ");
        let args: Vec<String> = arguments
            .iter()
            .map(|a| format!("{}: `{}`", a.name, a.ty))
            .collect();
        info.push_str(&args.join(", "));
        info.push_str("\n\n");
    }
    if let Some(desc) = description
        && !desc.trim().is_empty()
    {
        info.push_str(desc);
    }
    info
}

fn describe_operation_markdown(
    op_type: &str,
    op_name: Option<&str>,
    variables: &[(String, String)],
    description: Option<&str>,
) -> String {
    let mut info = format!("### {} {}\n", op_type, op_name.unwrap_or(""));
    info.push_str("---\n");
    if !variables.is_empty() {
        info.push_str("Variables: ");
        let vars: Vec<String> = variables
            .iter()
            .map(|(name, ty)| format!("{}: `{}`", name, ty))
            .collect();
        info.push_str(&vars.join(", "));
        info.push_str("\n\n");
    }
    if let Some(desc) = description {
        info.push_str(desc);
    }
    info
}

fn describe_variable_markdown(var_name: &str, var_type: &str) -> String {
    format!("### variable {}\n---\nType: `{}`", var_name, var_type)
}

fn describe_literal_markdown(kind: &str, expected_type: &str) -> String {
    let display_kind = match kind {
        "string_value" => "string value",
        "int_value" => "int value",
        "float_value" => "float value",
        "boolean_value" => "boolean value",
        "null_value" => "null value",
        _ => "value",
    };
    format!(
        "### {}\n---\nExpected type: `{}`",
        display_kind, expected_type
    )
}

fn describe_enum_value_markdown(
    enum_name: &str,
    value_name: &str,
    description: Option<&str>,
    deprecation_reason: Option<&str>,
) -> String {
    let mut info = format!(
        "### enum value {}\n---\nType: `{}`\n",
        value_name, enum_name
    );
    if let Some(desc) = description
        && !desc.trim().is_empty()
    {
        info.push('\n');
        info.push_str(desc);
        info.push('\n');
    }
    if let Some(reason) = deprecation_reason {
        info.push_str(&format!("\n**Deprecated:** {}\n", reason));
    }
    info
}
