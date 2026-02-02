use crate::document::DocumentState;
use crate::queries::*;
use apollo_compiler::{schema, Schema};
use tower_lsp::lsp_types::*;
use tree_sitter::{Node, QueryCursor, StreamingIterator};

impl DocumentState {
    pub fn get_completion_items(
        &self,
        position: Position,
        schema: &Schema,
        fragments: Vec<String>,
    ) -> Vec<CompletionItem> {
        let char_idx = self.rope.line_to_char(position.line as usize) + position.character as usize;
        let byte_offset = self.rope.char_to_byte(char_idx);

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();
            let tree_len = root.end_byte();

            if byte_offset >= offset && byte_offset <= offset + tree_len {
                if let Some(items) =
                    self.find_completions_in_tree(root, offset, byte_offset, schema, &fragments)
                {
                    return items;
                }
            }
        }
        Vec::new()
    }

    fn find_completions_in_tree(
        &self,
        root: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
        fragments: &[String],
    ) -> Option<Vec<CompletionItem>> {
        let query = GQL_COMPLETION_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_COMPLETION_QUERY).unwrap()
        });

        let mut cursor = QueryCursor::new();
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
                    if capture_name == "operation" {
                        if let Some(items) = self.complete_operation(
                            cap.node,
                            offset,
                            cursor_offset,
                            schema,
                            fragments,
                        ) {
                            return Some(items);
                        }
                    } else if capture_name == "fragment" {
                        if let Some(items) = self.complete_fragment(
                            cap.node,
                            offset,
                            cursor_offset,
                            schema,
                            fragments,
                        ) {
                            return Some(items);
                        }
                    } else if capture_name == "type_cond" {
                        return Some(self.get_all_type_completions(schema));
                    } else if capture_name == "frag_spread" {
                        return Some(self.get_fragment_name_completions(fragments));
                    } else if capture_name == "variable" || capture_name == "args" {
                        return Some(self.get_operation_variables(root, offset, cursor_offset));
                    }
                }
            }
        }
        None
    }

    fn get_operation_variables(
        &self,
        root: Node,
        offset: usize,
        cursor_offset: usize,
    ) -> Vec<CompletionItem> {
        // Find the operation definition containing the cursor
        let mut target_op = None;

        // We can't easily find it with simple walk if it's nested deep, but we can look for the node at byte
        let local_byte = cursor_offset - offset;
        let mut current = root.descendant_for_byte_range(local_byte, local_byte);

        while let Some(node) = current {
            if node.kind() == "operation_definition" {
                target_op = Some(node);
                break;
            }
            current = node.parent();
        }

        if let Some(op) = target_op {
            let mut variables = Vec::new();
            let mut walker = op.walk();
            for child in op.children(&mut walker) {
                if child.kind() == "variable_definitions" {
                    let mut def_walker = child.walk();
                    for def in child.children(&mut def_walker) {
                        if def.kind() == "variable_definition" {
                            let mut var_walker = def.walk();
                            for var_child in def.children(&mut var_walker) {
                                if var_child.kind() == "variable" {
                                    let name = self.get_node_text(var_child, offset);
                                    variables.push(CompletionItem {
                                        label: name,
                                        kind: Some(CompletionItemKind::VARIABLE),
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                    }
                }
            }
            return variables;
        }

        Vec::new()
    }

    fn complete_operation(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
        fragments: &[String],
    ) -> Option<Vec<CompletionItem>> {
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
                    return self.complete_selection_set_recursive(
                        node,
                        offset,
                        cursor_offset,
                        root_type,
                        schema,
                        fragments,
                    );
                }
            }
        }
        None
    }

    fn complete_fragment(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
        fragments: &[String],
    ) -> Option<Vec<CompletionItem>> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_condition" {
                let range = (child.start_byte() + offset)..(child.end_byte() + offset);
                if cursor_offset >= range.start && cursor_offset <= range.end {
                    return Some(self.get_all_type_completions(schema));
                }
            } else if child.kind() == "selection_set" {
                let range = (child.start_byte() + offset)..(child.end_byte() + offset);
                if cursor_offset >= range.start && cursor_offset <= range.end {
                    if let Some(type_name) = self.get_fragment_type_condition(node, offset) {
                        if let Some(type_def) = schema.types.get(type_name.as_str()) {
                            return self.complete_selection_set_recursive(
                                child,
                                offset,
                                cursor_offset,
                                type_def,
                                schema,
                                fragments,
                            );
                        }
                    }
                }
            }
        }
        None
    }

    fn complete_selection_set_recursive(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        fragments: &[String],
    ) -> Option<Vec<CompletionItem>> {
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
            if found.is_none() {
                return None;
            }
            found.unwrap()
        };

        let range = (target_node.start_byte() + offset)..(target_node.end_byte() + offset);
        if cursor_offset < range.start || cursor_offset > range.end {
            return None;
        }

        let mut cursor = target_node.walk();
        for child in target_node.children(&mut cursor) {
            let child_range = (child.start_byte() + offset)..(child.end_byte() + offset);
            if cursor_offset >= child_range.start && cursor_offset <= child_range.end {
                let kind = child.kind();
                if kind == "selection" {
                    let mut inner = child.walk();
                    for inner_child in child.children(&mut inner) {
                        if inner_child.kind() == "field" {
                            if let Some(items) = self.complete_field(
                                inner_child,
                                offset,
                                cursor_offset,
                                parent_type,
                                schema,
                                fragments,
                            ) {
                                return Some(items);
                            }
                        } else if inner_child.kind() == "fragment_spread" {
                            return Some(self.get_fragment_name_completions(fragments));
                        }
                    }
                } else if kind == "field" {
                    if let Some(items) = self.complete_field(
                        child,
                        offset,
                        cursor_offset,
                        parent_type,
                        schema,
                        fragments,
                    ) {
                        return Some(items);
                    }
                } else if kind == "fragment_spread" {
                    return Some(self.get_fragment_name_completions(fragments));
                }
            }
        }

        Some(self.get_field_completions(parent_type))
    }

    fn complete_field(
        &self,
        field_node: Node,
        offset: usize,
        cursor_offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        fragments: &[String],
    ) -> Option<Vec<CompletionItem>> {
        let mut field_name_node = None;
        let mut cursor_inner = field_node.walk();
        for child in field_node.children(&mut cursor_inner) {
            if child.kind() == "name" {
                field_name_node = Some(child);
                break;
            }
        }

        if let Some(field_name_node) = field_name_node {
            let field_name = self.get_node_text(field_name_node, offset);

            let field_def = match parent_type {
                schema::ExtendedType::Object(obj) => obj.fields.get(field_name.as_str()),
                schema::ExtendedType::Interface(iface) => iface.fields.get(field_name.as_str()),
                _ => None,
            };

            if let Some(field_def) = field_def {
                let mut sub_sel_set = None;
                let mut arguments_node = None;
                let mut f_cursor = field_node.walk();
                for f_child in field_node.children(&mut f_cursor) {
                    if f_child.kind() == "selection_set" {
                        sub_sel_set = Some(f_child);
                    } else if f_child.kind() == "arguments" {
                        arguments_node = Some(f_child);
                    }
                }

                if let Some(args) = arguments_node {
                    let args_range = (args.start_byte() + offset)..(args.end_byte() + offset);
                    if cursor_offset >= args_range.start && cursor_offset <= args_range.end {
                        return Some(self.get_operation_variables(
                            field_node,
                            offset,
                            cursor_offset,
                        ));
                    }
                }

                if let Some(sss) = sub_sel_set {
                    let sss_range = (sss.start_byte() + offset)..(sss.end_byte() + offset);
                    if cursor_offset >= sss_range.start && cursor_offset <= sss_range.end {
                        let field_type_name = field_def.ty.inner_named_type();
                        if let Some(field_type_def) = schema.types.get(field_type_name.as_str()) {
                            return self.complete_selection_set_recursive(
                                sss,
                                offset,
                                cursor_offset,
                                field_type_def,
                                schema,
                                fragments,
                            );
                        }
                    }
                }
            }
        }
        None
    }

    fn get_fragment_name_completions(&self, fragments: &[String]) -> Vec<CompletionItem> {
        fragments
            .iter()
            .map(|name| CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::SNIPPET),
                ..Default::default()
            })
            .collect()
    }

    fn get_field_completions(&self, parent_type: &schema::ExtendedType) -> Vec<CompletionItem> {
        let mut items = Vec::new();
        match parent_type {
            schema::ExtendedType::Object(obj) => {
                for (name, def) in &obj.fields {
                    items.push(CompletionItem {
                        label: name.to_string(),
                        kind: Some(CompletionItemKind::FIELD),
                        detail: Some(def.ty.to_string()),
                        ..Default::default()
                    });
                }
            }
            schema::ExtendedType::Interface(iface) => {
                for (name, def) in &iface.fields {
                    items.push(CompletionItem {
                        label: name.to_string(),
                        kind: Some(CompletionItemKind::FIELD),
                        detail: Some(def.ty.to_string()),
                        ..Default::default()
                    });
                }
            }
            _ => {}
        }
        items.push(CompletionItem {
            label: "__typename".to_string(),
            kind: Some(CompletionItemKind::FIELD),
            detail: Some("String!".to_string()),
            ..Default::default()
        });
        items
    }

    fn get_all_type_completions(&self, schema: &Schema) -> Vec<CompletionItem> {
        let mut items = Vec::new();
        for (name, def) in &schema.types {
            let kind = match def {
                schema::ExtendedType::Object(_) | schema::ExtendedType::Interface(_) => {
                    Some(CompletionItemKind::CLASS)
                }
                schema::ExtendedType::Enum(_) => Some(CompletionItemKind::ENUM),
                schema::ExtendedType::Union(_) => Some(CompletionItemKind::INTERFACE),
                schema::ExtendedType::Scalar(_) => Some(CompletionItemKind::STRUCT),
                schema::ExtendedType::InputObject(_) => Some(CompletionItemKind::STRUCT),
            };
            items.push(CompletionItem {
                label: name.to_string(),
                kind,
                ..Default::default()
            });
        }
        items
    }
}
