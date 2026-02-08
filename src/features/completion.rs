use crate::document::DocumentState;
use apollo_compiler::{Schema, ast, schema};
use tower_lsp::lsp_types::*;
use tree_sitter::Node;

#[derive(Clone)]
pub struct FragmentCompletionInfo {
    pub name: String,
    pub type_condition: String,
    pub description: Option<String>,
    pub import_path: Option<String>,
    pub is_public: bool,
    pub is_type_only: bool,
    pub uri: Url,
    pub package_root: Option<std::path::PathBuf>,
    pub used_variables: Vec<String>,
    pub used_fragments: Vec<String>,
    pub requirements: std::collections::BTreeMap<String, String>,
}

impl DocumentState {
    pub fn get_completion_items(
        &self,
        position: Position,
        schema: &Schema,
        fragments: Vec<FragmentCompletionInfo>,
    ) -> Vec<CompletionItem> {
        let byte_offset = self.position_to_byte(position);

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();
            let tree_len = root.end_byte();
            let block_end = offset + tree_len;

            // Allow a small overhang for host languages to handle cursors at the end of templates.
            let allowed_end = if self.language.is_host_language() {
                block_end.saturating_add(1)
            } else {
                block_end
            };

            if byte_offset >= offset
                && byte_offset <= allowed_end
                && let Some(items) =
                    self.find_completions_in_tree(root, offset, byte_offset, schema, &fragments)
            {
                return items;
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
        fragments: &[FragmentCompletionInfo],
    ) -> Option<Vec<CompletionItem>> {
        let root_end = root.end_byte();
        let local_byte = cursor_offset.saturating_sub(offset);
        let clamped_local = if local_byte > root_end && root_end > 0 {
            root_end
        } else {
            local_byte
        };

        let start_node =
            root.descendant_for_byte_range(clamped_local.saturating_sub(1), clamped_local);

        // Handle inline fragment type completion after '... on '
        if self.is_after_on(cursor_offset) {
            let context_node = start_node
                .and_then(|n| {
                    self.find_ancestor_by_kinds_internal(
                        n,
                        &["selection_set", "field", "inline_fragment", "selection"],
                    )
                })
                .or(Some(root));

            if let Some(context) = context_node
                && let Some(mut parent_type) =
                    self.find_parent_type_for_node(context, offset, schema)
            {
                // Fallback for broken trees where selection_set is attached to the wrong parent
                if (parent_type.name() == "Query"
                    || parent_type.name() == "Mutation"
                    || parent_type.name() == "Subscription")
                    && context.kind() == "selection_set"
                    && let Some(preceding_type) = self.find_preceding_field_type_internal(
                        context,
                        offset,
                        cursor_offset,
                        &parent_type,
                        schema,
                    )
                {
                    parent_type = preceding_type;
                }

                let has_selection_set = self.has_trailing_selection_set_internal(cursor_offset);
                return Some(self.get_applicable_type_completions(
                    &parent_type,
                    schema,
                    !has_selection_set,
                    cursor_offset,
                ));
            }
        }

        let mut curr = start_node;
        while let Some(node) = curr {
            // Try directive completions
            if (self.is_after_at(cursor_offset) || node.kind() == "directive")
                && let Some(items) = self.try_directive_completions(node, offset, schema)
            {
                return Some(items);
            }

            // Try fragment spreads after dots
            if self.is_after_dots(cursor_offset)
                && let Some(items) = self.complete_selection_set_at_node(
                    node,
                    offset,
                    cursor_offset,
                    schema,
                    fragments,
                )
            {
                let filtered: Vec<_> = items
                    .into_iter()
                    .filter(|i| i.kind == Some(CompletionItemKind::SNIPPET))
                    .collect();
                if !filtered.is_empty() {
                    return Some(filtered);
                }
            }

            // Try node-specific completions
            if let Some(items) =
                self.try_node_kind_completions(node, root, offset, cursor_offset, schema, fragments)
            {
                return Some(items);
            }

            curr = node.parent();
        }

        None
    }

    fn try_directive_completions(
        &self,
        current: Node,
        offset: usize,
        schema: &Schema,
    ) -> Option<Vec<CompletionItem>> {
        let context_node = self.find_directive_context_node(current, offset)?;
        let directive_location = self.find_directive_location(context_node, offset)?;
        Some(self.get_directive_completions(schema, directive_location))
    }

    fn find_directive_context_node<'a>(
        &self,
        current: Node<'a>,
        offset: usize,
    ) -> Option<Node<'a>> {
        let mut context_node = if current.kind() == "directive"
            || (current.kind() == "name" && current.parent().map(|p| p.kind()) == Some("directive"))
        {
            let dir_node = if current.kind() == "name" {
                current.parent().unwrap()
            } else {
                current
            };
            dir_node.parent()
        } else if current.kind() == "ERROR" && self.get_node_text(current, offset) == "@" {
            if let Some(prev) = current.prev_sibling() {
                Some(prev)
            } else {
                Some(current)
            }
        } else {
            Some(current)
        };

        context_node = context_node.and_then(|node| {
            self.skip_through_kinds(node, &["name", "fragment_name", "ERROR", "MISSING"])
        });

        context_node
    }

    fn find_directive_location<'a>(
        &self,
        mut p: Node<'a>,
        offset: usize,
    ) -> Option<ast::DirectiveLocation> {
        loop {
            if p.kind() == "selection" {
                let mut cursor = p.walk();
                for child in p.children(&mut cursor) {
                    if matches!(
                        child.kind(),
                        "field" | "fragment_spread" | "inline_fragment"
                    ) {
                        p = child;
                        break;
                    }
                }
            }

            let location = match p.kind() {
                "field" => Some(ast::DirectiveLocation::Field),
                "fragment_definition" => Some(ast::DirectiveLocation::FragmentDefinition),
                "inline_fragment" => Some(ast::DirectiveLocation::InlineFragment),
                "fragment_spread" => Some(ast::DirectiveLocation::FragmentSpread),
                "operation_definition" => Some(self.get_operation_directive_location(p, offset)),
                _ => None,
            };

            if location.is_some() {
                return location;
            }

            p = p.parent()?;
            if matches!(p.kind(), "selection_set" | "document") {
                return None;
            }
        }
    }

    fn get_operation_directive_location(
        &self,
        node: Node,
        offset: usize,
    ) -> ast::DirectiveLocation {
        let op_type_string = self.get_operation_type(node, offset);
        let op_type = match op_type_string.as_str() {
            "mutation" => ast::OperationType::Mutation,
            "subscription" => ast::OperationType::Subscription,
            _ => ast::OperationType::Query,
        };

        match op_type {
            ast::OperationType::Query => ast::DirectiveLocation::Query,
            ast::OperationType::Mutation => ast::DirectiveLocation::Mutation,
            ast::OperationType::Subscription => ast::DirectiveLocation::Subscription,
        }
    }

    fn try_node_kind_completions(
        &self,
        current: Node,
        root: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
    ) -> Option<Vec<CompletionItem>> {
        match current.kind() {
            "type_condition" | "named_type" => {
                if self.is_after_on(cursor_offset)
                    && let Some(inline_node) =
                        self.find_ancestor_by_kind(current, "inline_fragment")
                    && let Some(parent_type) =
                        self.find_parent_type_for_node(inline_node, offset, schema)
                {
                    let has_sel = self
                        .find_child_by_kind(inline_node, "selection_set")
                        .is_some();
                    return Some(self.get_applicable_type_completions(
                        &parent_type,
                        schema,
                        !has_sel,
                        cursor_offset,
                    ));
                }
                Some(self.get_all_type_completions(schema))
            }
            "variable" | "variable_definitions" => {
                Some(self.get_operation_variables(root, offset, cursor_offset))
            }
            "arguments" | "argument" | "(" | "ERROR" => {
                let mut items = self.get_operation_variables(root, offset, cursor_offset);

                let field_node = if current.kind() == "field" {
                    Some(current)
                } else {
                    self.find_ancestor_by_kind(current, "field")
                };

                // If no field ancestor, we might be right after a field name in a broken tree
                let field_node = if field_node.is_none()
                    && let Some(sel_set) = self.find_ancestor_by_kind(current, "selection_set")
                {
                    self.find_field_node_before_offset(sel_set, offset, cursor_offset)
                } else {
                    field_node
                };

                if let Some(field_node) = field_node
                    && let Some(parent_type) =
                        self.find_parent_type_for_node(field_node, offset, schema)
                {
                    let components = self.extract_field_components(field_node);
                    if let Some(name_node) = components.name {
                        let field_name = self.get_node_text(name_node, offset);
                        let field_def = match &parent_type {
                            schema::ExtendedType::Object(obj) => {
                                obj.fields.get(field_name.as_str())
                            }
                            schema::ExtendedType::Interface(iface) => {
                                iface.fields.get(field_name.as_str())
                            }
                            _ => None,
                        };

                        if let Some(fdef) = field_def {
                            // Check if we are at a value position
                            if let Some(expected_type) = self.find_expected_type_for_node(
                                current,
                                offset,
                                Some(cursor_offset),
                                schema,
                            ) {
                                match expected_type {
                                    schema::ExtendedType::Enum(enum_ty) => {
                                        for (name, def) in &enum_ty.values {
                                            items.push(CompletionItem {
                                                label: name.to_string(),
                                                kind: Some(CompletionItemKind::ENUM_MEMBER),
                                                documentation: def.description.as_ref().map(|d| {
                                                    Documentation::MarkupContent(MarkupContent {
                                                        kind: MarkupKind::Markdown,
                                                        value: d.to_string(),
                                                    })
                                                }),
                                                ..Default::default()
                                            });
                                        }
                                        return Some(items);
                                    }
                                    schema::ExtendedType::InputObject(input_obj) => {
                                        for (name, def) in &input_obj.fields {
                                            items.push(CompletionItem {
                                                label: name.to_string(),
                                                kind: Some(CompletionItemKind::FIELD),
                                                detail: Some(def.ty.to_string()),
                                                documentation: def.description.as_ref().map(|d| {
                                                    Documentation::MarkupContent(MarkupContent {
                                                        kind: MarkupKind::Markdown,
                                                        value: d.to_string(),
                                                    })
                                                }),
                                                ..Default::default()
                                            });
                                        }
                                        return Some(items);
                                    }
                                    _ => {}
                                }
                            }

                            for arg in &fdef.arguments {
                                items.push(CompletionItem {
                                    label: arg.name.to_string(),
                                    kind: Some(CompletionItemKind::FIELD),
                                    detail: Some(arg.ty.to_string()),
                                    documentation: arg.description.as_ref().map(|d| {
                                        Documentation::MarkupContent(MarkupContent {
                                            kind: MarkupKind::Markdown,
                                            value: d.to_string(),
                                        })
                                    }),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                } else if let Some(directive_node) =
                    self.find_ancestor_by_kind(current, "directive")
                {
                    let name_node = self.find_child_by_kind(directive_node, "name");
                    if let Some(name_node) = name_node {
                        let dir_name = self.get_node_text(name_node, offset);
                        if let Some(dir_def) = schema.directive_definitions.get(dir_name.as_str()) {
                            for arg in &dir_def.arguments {
                                items.push(CompletionItem {
                                    label: arg.name.to_string(),
                                    kind: Some(CompletionItemKind::FIELD),
                                    detail: Some(arg.ty.to_string()),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }
                if items.is_empty() && current.kind() == "ERROR" {
                    return None;
                }
                Some(items)
            }
            "object_value" | "object_field" => {
                let mut items = Vec::new();
                if let Some(expected_type) =
                    self.find_expected_type_for_node(current, offset, Some(cursor_offset), schema)
                    && let schema::ExtendedType::InputObject(input_obj) = expected_type
                {
                    for (name, def) in &input_obj.fields {
                        items.push(CompletionItem {
                            label: name.to_string(),
                            kind: Some(CompletionItemKind::FIELD),
                            detail: Some(def.ty.to_string()),
                            documentation: def.description.as_ref().map(|d| {
                                Documentation::MarkupContent(MarkupContent {
                                    kind: MarkupKind::Markdown,
                                    value: d.to_string(),
                                })
                            }),
                            ..Default::default()
                        });
                    }
                }
                Some(items)
            }
            "enum_value" | "value" => {
                if let Some(expected_type) =
                    self.find_expected_type_for_node(current, offset, Some(cursor_offset), schema)
                    && let schema::ExtendedType::Enum(enum_ty) = expected_type
                {
                    let mut items = Vec::new();
                    for (name, def) in &enum_ty.values {
                        items.push(CompletionItem {
                            label: name.to_string(),
                            kind: Some(CompletionItemKind::ENUM_MEMBER),
                            documentation: def.description.as_ref().map(|d| {
                                Documentation::MarkupContent(MarkupContent {
                                    kind: MarkupKind::Markdown,
                                    value: d.to_string(),
                                })
                            }),
                            ..Default::default()
                        });
                    }
                    return Some(items);
                }
                None
            }
            "fragment_spread" => {
                let parent_type = self.find_parent_type_for_node(current, offset, schema);
                Some(self.get_fragment_name_completions(fragments, parent_type.as_ref(), schema))
            }
            "fragment_definition" => {
                if self.is_after_on(cursor_offset) {
                    return Some(self.get_all_type_completions(schema));
                }
                self.complete_selection_set_at_node(
                    current,
                    offset,
                    cursor_offset,
                    schema,
                    fragments,
                )
            }
            "selection_set" | "operation_definition" => self.complete_selection_set_at_node(
                current,
                offset,
                cursor_offset,
                schema,
                fragments,
            ),
            _ => None,
        }
    }

    fn find_ancestor_by_kinds_internal<'a>(
        &self,
        node: Node<'a>,
        kinds: &[&str],
    ) -> Option<Node<'a>> {
        let mut curr = node;
        while let Some(parent) = curr.parent() {
            if kinds.contains(&parent.kind()) {
                return Some(parent);
            }
            curr = parent;
        }
        None
    }

    fn find_preceding_field_type_internal(
        &self,
        selection_set: Node,
        offset: usize,
        cursor_offset: usize,
        current_type: &schema::ExtendedType,
        schema: &Schema,
    ) -> Option<schema::ExtendedType> {
        let mut cursor = selection_set.walk();
        let mut last_field = None;
        for child in selection_set.children(&mut cursor) {
            let field_node = if child.kind() == "selection" {
                self.find_child_by_kind(child, "field")
            } else if child.kind() == "field" {
                Some(child)
            } else {
                None
            };

            if let Some(f) = field_node {
                if f.end_byte() + offset <= cursor_offset {
                    last_field = Some(f);
                } else {
                    break;
                }
            }
        }

        if let Some(field) = last_field
            && let Some(name_node) = self.extract_field_components(field).name
        {
            let field_name = self.get_node_text(name_node, offset);
            let field_def = match &current_type {
                schema::ExtendedType::Object(obj) => obj.fields.get(field_name.as_str()),
                schema::ExtendedType::Interface(iface) => iface.fields.get(field_name.as_str()),
                _ => None,
            };
            if let Some(fdef) = field_def {
                return schema
                    .types
                    .get(fdef.ty.inner_named_type().as_str())
                    .cloned();
            }
        }
        None
    }

    fn has_trailing_selection_set_internal(&self, cursor_offset: usize) -> bool {
        let remaining = self.rope.byte_slice(cursor_offset..).to_string();
        for c in remaining.chars() {
            if c.is_whitespace() {
                continue;
            }
            return c == '{';
        }
        false
    }

    fn find_field_node_before_offset<'a>(
        &self,
        selection_set: Node<'a>,
        offset: usize,
        cursor_offset: usize,
    ) -> Option<Node<'a>> {
        let mut cursor = selection_set.walk();
        let mut last_field = None;
        for child in selection_set.children(&mut cursor) {
            let field_node = if child.kind() == "selection" {
                self.find_child_by_kind(child, "field")
            } else if child.kind() == "field" {
                Some(child)
            } else {
                None
            };

            if let Some(f) = field_node {
                if f.start_byte() + offset < cursor_offset {
                    last_field = Some(f);
                } else {
                    break;
                }
            }
        }
        last_field
    }

    fn find_expected_type_for_node(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: Option<usize>,
        schema: &Schema,
    ) -> Option<schema::ExtendedType> {
        let mut curr = Some(node);
        while let Some(current_node) = curr {
            match current_node.kind() {
                "argument" | "arguments" => {
                    let arg_name = if current_node.kind() == "argument" {
                        self.find_child_by_kind(current_node, "name")
                            .map(|n| self.get_node_text(n, offset))
                    } else if let Some(co) = cursor_offset {
                        // In arguments node, find which argument we are in or after
                        let mut cursor = current_node.walk();
                        let mut last_arg = None;
                        for child in current_node.children(&mut cursor) {
                            if child.kind() == "argument" && child.start_byte() + offset < co {
                                last_arg = Some(child);
                            }
                        }

                        if let Some(arg) = last_arg {
                            // Check if we are at value position of this argument
                            let text = self.get_node_text(arg, offset);
                            if let Some(colon_idx) = text.find(':') {
                                let absolute_colon = arg.start_byte() + offset + colon_idx;
                                if co > absolute_colon {
                                    self.find_child_by_kind(arg, "name")
                                        .map(|n| self.get_node_text(n, offset))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            // If no argument node found, we might be right after a name that isn't yet an argument
                            let text_before = self
                                .rope
                                .byte_slice(current_node.start_byte() + offset..co)
                                .to_string();
                            if let Some(colon_idx) = text_before.rfind(':') {
                                let name_part = &text_before[..colon_idx];
                                name_part
                                    .split(|c: char| !c.is_alphanumeric() && c != '_')
                                    .rfind(|s| !s.is_empty())
                                    .map(|s| s.to_string())
                            } else {
                                None
                            }
                        }
                    } else {
                        None
                    };

                    if let Some(arg_name) = arg_name {
                        let context_node = if current_node.kind() == "arguments" {
                            current_node
                        } else {
                            current_node.parent()?
                        };
                        let target_node = context_node.parent()?; // field or directive

                        if target_node.kind() == "field" {
                            let parent_type =
                                self.find_parent_type_for_node(target_node, offset, schema)?;
                            let field_name_node = self.extract_field_components(target_node).name?;
                            let field_name = self.get_node_text(field_name_node, offset);

                            let field_def = match &parent_type {
                                schema::ExtendedType::Object(obj) => {
                                    obj.fields.get(field_name.as_str())
                                }
                                schema::ExtendedType::Interface(iface) => {
                                    iface.fields.get(field_name.as_str())
                                }
                                _ => None,
                            }?;

                            let arg_def = field_def
                                .arguments
                                .iter()
                                .find(|a| a.name.as_str() == arg_name)?;
                            return schema
                                .types
                                .get(arg_def.ty.inner_named_type().as_str())
                                .cloned();
                        } else if target_node.kind() == "directive" {
                            let name_node = self.find_child_by_kind(target_node, "name")?;
                            let dir_name = self.get_node_text(name_node, offset);
                            let dir_def = schema.directive_definitions.get(dir_name.as_str())?;
                            let arg_def = dir_def
                                .arguments
                                .iter()
                                .find(|a| a.name.as_str() == arg_name)?;
                            return schema
                                .types
                                .get(arg_def.ty.inner_named_type().as_str())
                                .cloned();
                        }
                    }
                }
                "object_field" | "object_value" => {
                    let field_node = if current_node.kind() == "object_field" {
                        Some(current_node)
                    } else if let Some(co) = cursor_offset {
                        // In object_value node, find which field we are in or after
                        let mut cursor = current_node.walk();
                        let mut last_f = None;
                        for child in current_node.children(&mut cursor) {
                            if child.kind() == "object_field" && child.start_byte() + offset < co {
                                last_f = Some(child);
                            }
                        }
                        last_f
                    } else {
                        None
                    };

                    if let Some(f) = field_node {
                        // If we have an offset, check if we are actually at the value position
                        if let Some(co) = cursor_offset {
                            let text = self.get_node_text(f, offset);
                            if let Some(colon_idx) = text.find(':') {
                                let absolute_colon = f.start_byte() + offset + colon_idx;
                                if co <= absolute_colon {
                                    // We are still at the name part of the field
                                    return None;
                                }
                            } else {
                                // No colon found in the field node yet
                                return None;
                            }
                        }

                        let field_name_node = self.find_child_by_kind(f, "name")?;
                        let field_name = self.get_node_text(field_name_node, offset);

                        let object_value_node = f.parent()?;
                        let parent_input_type = self.find_expected_type_for_node(
                            object_value_node,
                            offset,
                            cursor_offset,
                            schema,
                        )?;

                        if let schema::ExtendedType::InputObject(input_obj) = parent_input_type {
                            let field_def = input_obj.fields.get(field_name.as_str())?;
                            return schema
                                .types
                                .get(field_def.ty.inner_named_type().as_str())
                                .cloned();
                        }
                    }
                }
                "list_value" => {
                    // Recurse to find the type of the list itself
                    let list_type = self.find_expected_type_for_node(
                        current_node,
                        offset,
                        cursor_offset,
                        schema,
                    )?;
                    return Some(list_type);
                }
                _ => {}
            }
            curr = current_node.parent();
        }
        None
    }

    fn complete_selection_set_at_node(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
    ) -> Option<Vec<CompletionItem>> {
        match node.kind() {
            "operation_definition" => {
                self.complete_operation(node, offset, cursor_offset, schema, fragments)
            }
            "fragment_definition" => {
                self.complete_fragment(node, offset, cursor_offset, schema, fragments)
            }
            "inline_fragment" => {
                self.complete_inline_fragment(node, offset, cursor_offset, schema, fragments)
            }
            "selection_set" => {
                if let Some(parent) = node.parent() {
                    match parent.kind() {
                        "operation_definition" => {
                            return self.complete_operation(
                                parent,
                                offset,
                                cursor_offset,
                                schema,
                                fragments,
                            );
                        }
                        "fragment_definition" => {
                            return self.complete_fragment(
                                parent,
                                offset,
                                cursor_offset,
                                schema,
                                fragments,
                            );
                        }
                        "inline_fragment" => {
                            return self.complete_inline_fragment(
                                parent,
                                offset,
                                cursor_offset,
                                schema,
                                fragments,
                            );
                        }
                        "field" => {
                            if let Some(containing_type) =
                                self.find_parent_type_for_node(parent, offset, schema)
                                && let Some(field_name_node) =
                                    self.extract_field_components(parent).name
                            {
                                let field_name = self.get_node_text(field_name_node, offset);
                                let field_def = match &containing_type {
                                    schema::ExtendedType::Object(obj) => {
                                        obj.fields.get(field_name.as_str())
                                    }
                                    schema::ExtendedType::Interface(iface) => {
                                        iface.fields.get(field_name.as_str())
                                    }
                                    _ => None,
                                };

                                if let Some(fdef) = field_def
                                    && let Some(field_type_def) =
                                        schema.types.get(fdef.ty.inner_named_type().as_str())
                                {
                                    return self.complete_selection_set_recursive(
                                        node,
                                        offset,
                                        cursor_offset,
                                        field_type_def,
                                        schema,
                                        fragments,
                                    );
                                }
                            }
                            return None;
                        }
                        _ => return None,
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn get_operation_variables(
        &self,
        root: Node,
        offset: usize,
        cursor_offset: usize,
    ) -> Vec<CompletionItem> {
        let local_byte = cursor_offset.saturating_sub(offset);
        let current = root.descendant_for_byte_range(local_byte.saturating_sub(1), local_byte);

        let target_op = current.and_then(|c| self.find_ancestor_by_kind(c, "operation_definition"));

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
        fragments: &[FragmentCompletionInfo],
    ) -> Option<Vec<CompletionItem>> {
        let operation_type_string = self.get_operation_type(node, offset);

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
            return self.complete_selection_set_recursive(
                node,
                offset,
                cursor_offset,
                root_type,
                schema,
                fragments,
            );
        }
        None
    }

    fn complete_fragment(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
    ) -> Option<Vec<CompletionItem>> {
        if let Some(type_cond) = self.find_child_by_kind(node, "type_condition")
            && self.is_cursor_in_node_range(type_cond, offset, cursor_offset)
        {
            return Some(self.get_all_type_completions(schema));
        }

        if let Some(selection_set) = self.find_child_by_kind(node, "selection_set")
            && self.is_cursor_in_node_range(selection_set, offset, cursor_offset)
            && let Some(type_name) = self.get_fragment_type_condition(node, offset)
            && let Some(type_def) = schema.types.get(type_name.as_str())
        {
            return self.complete_selection_set_recursive(
                selection_set,
                offset,
                cursor_offset,
                type_def,
                schema,
                fragments,
            );
        }
        None
    }

    fn complete_inline_fragment(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
    ) -> Option<Vec<CompletionItem>> {
        let type_name = self.get_fragment_type_condition(node, offset);
        let parent_type = if let Some(tn) = type_name {
            schema.types.get(tn.as_str()).cloned()
        } else {
            let mut current = node.parent()?;
            while current.kind() != "selection_set" {
                current = current.parent()?;
            }
            self.find_parent_type_for_node(node, offset, schema)
        };

        if let Some(type_def) = parent_type
            && let Some(selection_set) = self.find_child_by_kind(node, "selection_set")
        {
            return self.complete_selection_set_recursive(
                selection_set,
                offset,
                cursor_offset,
                &type_def,
                schema,
                fragments,
            );
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
        fragments: &[FragmentCompletionInfo],
    ) -> Option<Vec<CompletionItem>> {
        let target_node = if node.kind() == "selection_set" {
            node
        } else {
            self.find_child_by_kind(node, "selection_set")?
        };

        if !self.is_cursor_in_node_range(target_node, offset, cursor_offset) {
            return None;
        }

        let mut cursor = target_node.walk();
        for child in target_node.children(&mut cursor) {
            if self.is_cursor_in_node_range(child, offset, cursor_offset) {
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
                        } else if inner_child.kind() == "fragment_spread"
                            || inner_child.kind() == "..."
                        {
                            return Some(self.get_fragment_name_completions(
                                fragments,
                                Some(parent_type),
                                schema,
                            ));
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
                } else if kind == "fragment_spread" || kind == "..." {
                    return Some(self.get_fragment_name_completions(
                        fragments,
                        Some(parent_type),
                        schema,
                    ));
                }
            }
        }

        Some(self.get_field_completions(parent_type, schema))
    }

    fn complete_field(
        &self,
        field_node: Node,
        offset: usize,
        cursor_offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
    ) -> Option<Vec<CompletionItem>> {
        let components = self.extract_field_components(field_node);

        if let Some(field_name_node) = components.name {
            let field_name = self.get_node_text(field_name_node, offset);

            let field_def = match parent_type {
                schema::ExtendedType::Object(obj) => obj.fields.get(field_name.as_str()),
                schema::ExtendedType::Interface(iface) => iface.fields.get(field_name.as_str()),
                _ => None,
            };

            if let Some(field_def) = field_def {
                if let Some(args) = components.arguments
                    && self.is_cursor_in_node_range(args, offset, cursor_offset)
                {
                    return Some(self.get_operation_variables(field_node, offset, cursor_offset));
                }

                if let Some(sss) = components.selection_set
                    && self.is_cursor_in_node_range(sss, offset, cursor_offset)
                {
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
        None
    }

    fn get_fragment_name_completions(
        &self,
        fragments: &[FragmentCompletionInfo],
        expected_type: Option<&schema::ExtendedType>,
        schema: &Schema,
    ) -> Vec<CompletionItem> {
        fragments
            .iter()
            .filter(|f| {
                if let Some(parent) = expected_type {
                    let parent_name = parent.name();
                    if f.type_condition == parent_name.as_str() {
                        return true;
                    }

                    match parent {
                        schema::ExtendedType::Object(obj) => {
                            if obj
                                .implements_interfaces
                                .iter()
                                .any(|i| i.as_str() == f.type_condition)
                            {
                                return true;
                            }
                        }
                        schema::ExtendedType::Interface(iface) => {
                            if iface
                                .implements_interfaces
                                .iter()
                                .any(|i| i.as_str() == f.type_condition)
                            {
                                return true;
                            }
                        }
                        schema::ExtendedType::Union(union) => {
                            if union.members.iter().any(|m| m.as_str() == f.type_condition) {
                                return true;
                            }
                        }
                        _ => {}
                    }

                    if let Some(frag_type) = schema.types.get(f.type_condition.as_str())
                        && let schema::ExtendedType::Union(u) = frag_type
                        && u.members.iter().any(|m| m.as_str() == parent_name.as_str())
                    {
                        return true;
                    }
                    false
                } else {
                    true
                }
            })
            .map(|f| {
                let mut documentation = f.description.clone().unwrap_or_default();
                if !f.requirements.is_empty() {
                    if !documentation.is_empty() {
                        documentation.push_str("\n\n---\n");
                    }
                    documentation.push_str("**Requires Variables:**\n");
                    for (var, ty) in &f.requirements {
                        documentation.push_str(&format!("- `${}`: `{}`\n", var, ty));
                    }
                }
                if let Some(import) = &f.import_path {
                    if !documentation.is_empty() {
                        documentation.push_str("\n\n---\n");
                    }
                    documentation.push_str(&format!("Import: `{}`", import));
                }
                CompletionItem {
                    label: f.name.clone(),
                    kind: Some(CompletionItemKind::SNIPPET),
                    documentation: if documentation.is_empty() {
                        None
                    } else {
                        Some(Documentation::MarkupContent(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: documentation,
                        }))
                    },
                    ..Default::default()
                }
            })
            .collect()
    }

    fn get_field_completions(
        &self,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
    ) -> Vec<CompletionItem> {
        let mut items = Vec::new();
        match parent_type {
            schema::ExtendedType::Object(obj) => {
                for (name, def) in &obj.fields {
                    items.push(CompletionItem {
                        label: name.to_string(),
                        kind: Some(CompletionItemKind::FIELD),
                        detail: Some(def.ty.to_string()),
                        documentation: def.description.as_ref().map(|d| {
                            Documentation::MarkupContent(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: d.to_string(),
                            })
                        }),
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
                        documentation: def.description.as_ref().map(|d| {
                            Documentation::MarkupContent(MarkupContent {
                                kind: MarkupKind::Markdown,
                                value: d.to_string(),
                            })
                        }),
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
        if Self::is_query_root(parent_type, schema) {
            items.push(CompletionItem {
                label: "__schema".to_string(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some("__Schema!".to_string()),
                documentation: Some(Documentation::String(
                    "Access the current schema introspection object.".to_string(),
                )),
                ..Default::default()
            });
            items.push(CompletionItem {
                label: "__type".to_string(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some("__Type".to_string()),
                documentation: Some(Documentation::String(
                    "Look up a type definition by its name.".to_string(),
                )),
                ..Default::default()
            });
        }
        items
    }

    fn is_query_root(ty: &schema::ExtendedType, schema: &Schema) -> bool {
        schema
            .root_operation(ast::OperationType::Query)
            .and_then(|root_name| schema.types.get(root_name.as_str()))
            .map(|root_type| root_type.name() == ty.name())
            .unwrap_or(false)
    }

    fn get_all_type_completions(&self, schema: &Schema) -> Vec<CompletionItem> {
        let mut items = Vec::new();
        for (name, def) in &schema.types {
            if name.starts_with("__") {
                continue;
            }
            let kind = match def {
                schema::ExtendedType::Object(_) => Some(CompletionItemKind::CLASS),
                schema::ExtendedType::Interface(_) => Some(CompletionItemKind::INTERFACE),
                schema::ExtendedType::Enum(_) => Some(CompletionItemKind::ENUM),
                schema::ExtendedType::Union(_) => Some(CompletionItemKind::INTERFACE),
                schema::ExtendedType::Scalar(_) => Some(CompletionItemKind::STRUCT),
                schema::ExtendedType::InputObject(_) => Some(CompletionItemKind::STRUCT),
            };
            items.push(CompletionItem {
                label: name.to_string(),
                kind,
                documentation: def.description().map(|d| {
                    Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: d.to_string(),
                    })
                }),
                ..Default::default()
            });
        }
        items
    }

    fn get_applicable_type_completions(
        &self,
        parent: &schema::ExtendedType,
        schema: &Schema,
        add_braces: bool,
        cursor_offset: usize,
    ) -> Vec<CompletionItem> {
        let mut items = Vec::new();
        for (name, def) in &schema.types {
            if name.starts_with("__") {
                continue;
            }
            let mut include = false;
            match parent {
                schema::ExtendedType::Object(obj) => {
                    if obj.name.as_str() == name.as_str() {
                        include = true;
                    }
                    if obj
                        .implements_interfaces
                        .iter()
                        .any(|i| i.as_str() == name.as_str())
                    {
                        include = true;
                    }
                    if let schema::ExtendedType::Union(u) = def
                        && u.members.iter().any(|m| m.as_str() == obj.name.as_str())
                    {
                        include = true;
                    }
                }
                schema::ExtendedType::Interface(iface) => {
                    if let schema::ExtendedType::Object(o) = def
                        && o.implements_interfaces
                            .iter()
                            .any(|i| i.as_str() == iface.name.as_str())
                    {
                        include = true;
                    }
                    if iface.name.as_str() == name.as_str() {
                        include = true;
                    }
                    if let schema::ExtendedType::Interface(subiface) = def
                        && subiface
                            .implements_interfaces
                            .iter()
                            .any(|i| i.as_str() == iface.name.as_str())
                    {
                        include = true;
                    }
                }
                schema::ExtendedType::Union(u) => {
                    if u.members.iter().any(|m| m.as_str() == name.as_str()) {
                        include = true;
                    }
                    if u.name.as_str() == name.as_str() {
                        include = true;
                    }
                }
                _ => {}
            }

            if include {
                let kind = match def {
                    schema::ExtendedType::Object(_) => Some(CompletionItemKind::CLASS),
                    schema::ExtendedType::Interface(_) => Some(CompletionItemKind::INTERFACE),
                    schema::ExtendedType::Enum(_) => Some(CompletionItemKind::ENUM),
                    schema::ExtendedType::Union(_) => Some(CompletionItemKind::INTERFACE),
                    schema::ExtendedType::Scalar(_) => Some(CompletionItemKind::STRUCT),
                    schema::ExtendedType::InputObject(_) => Some(CompletionItemKind::STRUCT),
                };

                let mut item = CompletionItem {
                    label: name.to_string(),
                    kind,
                    documentation: def.description().map(|d| {
                        Documentation::MarkupContent(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: d.to_string(),
                        })
                    }),
                    ..Default::default()
                };

                let (_prefix_len, start_offset) = self.get_prefix_at_cursor(cursor_offset);
                let start_pos = self.byte_to_position(start_offset);
                let end_pos = self.byte_to_position(cursor_offset);

                if add_braces {
                    let line_idx = self.rope.byte_to_line(cursor_offset);
                    let line_start = self.rope.line_to_byte(line_idx);
                    let line_slice = self.rope.byte_slice(line_start..cursor_offset).to_string();
                    let mut indent = String::new();
                    for c in line_slice.chars() {
                        if c.is_whitespace() {
                            indent.push(c);
                        } else {
                            break;
                        }
                    }
                    let snippet = format!("{} {{\n{}  $0\n{}}}", name, indent, indent);
                    item.insert_text = Some(snippet.clone());
                    item.insert_text_format = Some(InsertTextFormat::SNIPPET);
                    item.text_edit = Some(CompletionTextEdit::Edit(TextEdit {
                        range: Range {
                            start: start_pos,
                            end: end_pos,
                        },
                        new_text: snippet.replace("$0", ""),
                    }));
                } else {
                    item.text_edit = Some(CompletionTextEdit::Edit(TextEdit {
                        range: Range {
                            start: start_pos,
                            end: end_pos,
                        },
                        new_text: name.to_string(),
                    }));
                }
                items.push(item);
            }
        }
        items
    }

    fn get_prefix_at_cursor(&self, cursor_offset: usize) -> (usize, usize) {
        let max_scan = 64usize;
        let search_start = cursor_offset.saturating_sub(max_scan);
        let slice = self
            .rope
            .byte_slice(search_start..cursor_offset)
            .to_string();
        let bytes = slice.as_bytes();
        let mut i = bytes.len();
        while i > 0 {
            let b = bytes[i - 1];
            if b == b'_' || b.is_ascii_alphanumeric() {
                i -= 1;
                continue;
            }
            break;
        }
        (bytes.len() - i, search_start + i)
    }

    fn get_directive_completions(
        &self,
        schema: &Schema,
        location: ast::DirectiveLocation,
    ) -> Vec<CompletionItem> {
        let mut items = Vec::new();
        for (name, def) in &schema.directive_definitions {
            if def.locations.contains(&location) {
                items.push(CompletionItem {
                    label: name.to_string(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    documentation: def.description.as_ref().map(|d| {
                        Documentation::MarkupContent(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: d.to_string(),
                        })
                    }),
                    ..Default::default()
                });
            }
        }
        if matches!(
            location,
            ast::DirectiveLocation::FragmentDefinition
                | ast::DirectiveLocation::InlineFragment
                | ast::DirectiveLocation::FragmentSpread
        ) {
            if !items.iter().any(|i| i.label == "public") {
                items.push(CompletionItem {
                    label: "public".to_string(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    documentation: Some(Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: "Marks the fragment as public for codegen".to_string(),
                    })),
                    ..Default::default()
                });
            }
            if !items.iter().any(|i| i.label == "type_only") {
                items.push(CompletionItem {
                    label: "type_only".to_string(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    documentation: Some(Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: "Marks the fragment as type-only for codegen".to_string(),
                    })),
                    ..Default::default()
                });
            }
        }
        items
    }

    fn is_after_at(&self, cursor_offset: usize) -> bool {
        if cursor_offset == 0 {
            return false;
        }
        self.rope.char(self.rope.byte_to_char(cursor_offset - 1)) == '@'
    }

    fn is_after_dots(&self, cursor_offset: usize) -> bool {
        let mut dot_count = 0;
        let mut curr = cursor_offset;
        while curr > 0 {
            let c = self.rope.char(self.rope.byte_to_char(curr - 1));
            if c.is_whitespace() {
                curr -= 1;
                continue;
            }
            if c == '.' {
                dot_count += 1;
                curr -= 1;
                if dot_count == 3 {
                    return true;
                }
                continue;
            }
            break;
        }
        false
    }

    fn is_after_on(&self, cursor_offset: usize) -> bool {
        let mut found_n = false;
        let mut curr = cursor_offset;
        while curr > 0 {
            let c = self.rope.char(self.rope.byte_to_char(curr - 1));
            if c.is_whitespace() {
                curr -= 1;
                continue;
            }
            if !found_n {
                if c == 'n' || c == 'N' {
                    found_n = true;
                    curr -= 1;
                    continue;
                }
                return false;
            } else {
                if c == 'o' || c == 'O' {
                    if curr > 1 {
                        let prev = self.rope.char(self.rope.byte_to_char(curr - 2));
                        return !self.is_name_char(prev);
                    }
                    return true;
                }
                return false;
            }
        }
        false
    }

    fn is_name_char(&self, c: char) -> bool {
        c == '_' || c.is_ascii_alphanumeric()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::DocumentState;
    use tree_sitter::Parser;

    fn create_doc(src: &str) -> DocumentState {
        let uri = Url::parse("file:///tmp/test.tsx").unwrap();
        let mut parser = Parser::new();
        let lang: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TSX.into();
        parser.set_language(&lang).unwrap();
        DocumentState::new(uri, src, parser)
    }

    #[test]
    fn test_is_after_on() {
        let doc = create_doc("const q = graphql(`... on `);");
        let offset = doc.get_graphql_trees()[0].offset;
        assert!(doc.is_after_on(offset + 7)); // After 'on '
        assert!(doc.is_after_on(offset + 6)); // Exactly after 'on'

        let doc2 = create_doc("const q = graphql(`person `);");
        let offset2 = doc2.get_graphql_trees()[0].offset;
        assert!(!doc2.is_after_on(offset2 + 7)); // Should NOT match 'person'
    }

    #[test]
    fn test_is_after_dots() {
        let doc = create_doc("const q = graphql(`...`);");
        let offset = doc.get_graphql_trees()[0].offset;
        assert!(doc.is_after_dots(offset + 3));
        assert!(!doc.is_after_dots(offset + 2));
    }
}
