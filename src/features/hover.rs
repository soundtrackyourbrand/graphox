use crate::document::DocumentState;
use crate::queries::*;
use apollo_compiler::{schema, Schema};
use tower_lsp::lsp_types::*;
use tree_sitter::{Node, StreamingIterator};

impl DocumentState {
    pub fn get_hover_info(&self, position: Position, schema: &Schema) -> Option<Hover> {
        let byte_offset = self.position_to_byte(position);

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();
            let tree_len = root.end_byte();

            if byte_offset >= offset && byte_offset < offset + tree_len {
                let local_byte = byte_offset - offset;
                let node = root.descendant_for_byte_range(local_byte, local_byte)?;

                if node.kind() == "name" {
                    let symbol_name = self
                        .rope
                        .slice(
                            self.rope.byte_to_char(node.start_byte() + offset)
                                ..self.rope.byte_to_char(node.end_byte() + offset),
                        )
                        .to_string();

                    if let Some(schema_info) = self.get_type_info_from_schema(&symbol_name, schema) {
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

                    if let Some(field_info) = self.get_field_info(root, offset, byte_offset, schema) {
                        return Some(Hover {
                            contents: HoverContents::Markup(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: field_info,
                            }),
                            range: Some(self.translate_to_file_range(node, offset)),
                        });
                    }
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
                            )
                        }
                        "fragment" => {
                            return self.find_field_in_fragment(
                                cap.node,
                                offset,
                                cursor_offset,
                                schema,
                            )
                        }
                        _ => {}
                    }
                }
            }
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

        if let Some(op) = op_type
            && let Some(root_def_name) = schema.root_operation(op)
            && let Some(root_type) = schema.types.get(root_def_name.as_str())
        {
            return self.find_field_recursive(node, offset, cursor_offset, root_type, schema);
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
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "selection_set" {
                let range = (child.start_byte() + offset)..(child.end_byte() + offset);
                if cursor_offset >= range.start && cursor_offset <= range.end
                    && let Some(type_name) = self.get_fragment_type_condition(node, offset)
                    && let Some(type_def) = schema.types.get(type_name.as_str())
                {
                    return self.find_field_recursive(
                        child,
                        offset,
                        cursor_offset,
                        type_def,
                        schema,
                    );
                }
            }
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
    ) -> Option<String> {
        let target_node = if node.kind() == "selection_set" {
            node
        } else {
            let mut found = None;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "selection_set" {
                    found = Some(child);
                    break;
                }
            }
            found?
        };

        let mut cursor = target_node.walk();
        for child in target_node.children(&mut cursor) {
            let child_range = (child.start_byte() + offset)..(child.end_byte() + offset);
            if cursor_offset >= child_range.start && cursor_offset <= child_range.end {
                let kind = child.kind();
                if kind == "selection" {
                    let mut inner = child.walk();
                    for inner_child in child.children(&mut inner) {
                        if inner_child.kind() == "field"
                            && let Some(info) = self.find_field_info(
                                inner_child,
                                offset,
                                cursor_offset,
                                parent_type,
                                schema,
                            )
                        {
                            return Some(info);
                        }
                    }
                } else if kind == "field"
                    && let Some(info) =
                        self.find_field_info(child, offset, cursor_offset, parent_type, schema)
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
    ) -> Option<String> {
        let mut name_node = None;
        let mut selection_set_node = None;

        let mut cursor = field_node.walk();
        for child in field_node.children(&mut cursor) {
            match child.kind() {
                "name" => name_node = Some(child),
                "selection_set" => selection_set_node = Some(child),
                _ => {}
            }
        }

        if let Some(name_node) = name_node {
            let field_name = self.get_node_text(name_node, offset);
            let name_range = (name_node.start_byte() + offset)..(name_node.end_byte() + offset);

            let field_def = match parent_type {
                schema::ExtendedType::Object(obj) => obj.fields.get(field_name.as_str()),
                schema::ExtendedType::Interface(iface) => iface.fields.get(field_name.as_str()),
                _ => None,
            };

            if let Some(field_def) = field_def {
                if cursor_offset >= name_range.start && cursor_offset <= name_range.end {
                    let mut info =
                        format!("### field {}.{}\n---\n", parent_type.name(), field_name);
                    info.push_str(&format!("Type: `{}`\n", field_def.ty));
                    if let Some(desc) = &field_def.description {
                        info.push('\n');
                        info.push_str(desc);
                    }
                    return Some(info);
                }

                if let Some(sss) = selection_set_node {
                    let sss_range = (sss.start_byte() + offset)..(sss.end_byte() + offset);
                    if cursor_offset >= sss_range.start && cursor_offset <= sss_range.end {
                        let field_type_name = field_def.ty.inner_named_type();
                        if let Some(field_type_def) = schema.types.get(field_type_name.as_str()) {
                            return self.find_field_recursive(
                                sss,
                                offset,
                                cursor_offset,
                                field_type_def,
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
                let mut desc_node = None;
                let mut name_node = None;

                for capture in m.captures {
                    let capture_name = query.capture_names()[capture.index as usize];
                    if capture_name == "desc" {
                        desc_node = Some(capture.node);
                    } else if capture_name == "name" {
                        name_node = Some(capture.node);
                    }
                }

                if let Some(n_node) = name_node {
                    let name = self
                        .rope
                        .slice(
                            self.rope.byte_to_char(n_node.start_byte() + offset)
                                ..self.rope.byte_to_char(n_node.end_byte() + offset),
                        )
                        .to_string();

                    if name == target_name {
                        if let Some(d_node) = desc_node {
                            return Some(
                                self.rope
                                    .slice(
                                        self.rope.byte_to_char(d_node.start_byte() + offset)
                                            ..self.rope.byte_to_char(d_node.end_byte() + offset),
                                    )
                                    .to_string()
                                    .trim_matches('"')
                                    .to_string(),
                            );
                        } else {
                            return None;
                        }
                    }
                }
            }
        }
        None
    }
}
