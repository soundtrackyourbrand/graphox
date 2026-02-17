use apollo_compiler::{Schema, ast, schema};
use graphox_core::document::DocumentState;
use lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat, Position, TextEdit};
use tree_sitter::Node;

pub mod constants;
pub mod cursor;
pub mod directives;
pub mod fields;
pub mod fragments;
pub mod keywords;
pub mod operations;
pub mod types;
pub mod utils;
pub mod values;

pub use types::{FragmentCompletionInfo, FragmentRequirements, FragmentRequirementsResolver};

fn is_cursor_inside_comment(
    doc: &DocumentState,
    root: Node,
    offset: usize,
    cursor_offset: usize,
) -> bool {
    let root_end = root.end_byte();
    let local_byte = cursor_offset.saturating_sub(offset);
    let clamped_local = if local_byte > root_end && root_end > 0 {
        root_end
    } else {
        local_byte
    };

    if let Some(current) =
        root.descendant_for_byte_range(clamped_local.saturating_sub(1), clamped_local)
    {
        return current.kind() == "comment"
            || doc.find_ancestor_by_kind(current, "comment").is_some();
    }

    false
}

#[allow(clippy::too_many_arguments)]
pub trait DocumentCompletion {
    fn get_completion_items(
        &self,
        position: Position,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Vec<CompletionItem>;

    fn find_completions_in_tree(
        &self,
        root: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Option<Vec<CompletionItem>>;

    fn try_directive_completions(
        &self,
        current: Node,
        offset: usize,
        schema: &Schema,
    ) -> Option<Vec<CompletionItem>>;

    fn find_directive_context_node<'a>(&self, current: Node<'a>, offset: usize)
    -> Option<Node<'a>>;

    fn find_directive_location<'a>(
        &self,
        p: Node<'a>,
        offset: usize,
    ) -> Option<ast::DirectiveLocation>;

    fn get_operation_directive_location(&self, node: Node, offset: usize)
    -> ast::DirectiveLocation;

    fn try_node_kind_completions(
        &self,
        current: Node,
        root: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Option<Vec<CompletionItem>>;

    fn find_preceding_field_type_internal(
        &self,
        selection_set: Node,
        offset: usize,
        cursor_offset: usize,
        current_type: &schema::ExtendedType,
        schema: &Schema,
    ) -> Option<schema::ExtendedType>;

    fn has_trailing_selection_set_internal(&self, cursor_offset: usize) -> bool;

    fn find_field_node_before_offset<'a>(
        &self,
        selection_set: Node<'a>,
        offset: usize,
        cursor_offset: usize,
    ) -> Option<Node<'a>>;

    fn find_expected_type_for_node(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: Option<usize>,
        schema: &Schema,
    ) -> Option<schema::ExtendedType>;

    fn find_expected_ast_type_for_node(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: Option<usize>,
        schema: &Schema,
    ) -> Option<ast::Type>;

    fn complete_selection_set_at_node(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Option<Vec<CompletionItem>>;

    fn get_operation_variables(
        &self,
        root: Node,
        offset: usize,
        cursor_offset: usize,
    ) -> Vec<CompletionItem>;

    fn complete_operation(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Option<Vec<CompletionItem>>;

    fn complete_fragment(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Option<Vec<CompletionItem>>;

    fn complete_inline_fragment(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Option<Vec<CompletionItem>>;

    fn complete_selection_set_recursive(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Option<Vec<CompletionItem>>;

    fn complete_field(
        &self,
        field_node: Node,
        offset: usize,
        cursor_offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Option<Vec<CompletionItem>>;

    fn get_fragment_name_completions(
        &self,
        fragments: &[FragmentCompletionInfo],
        expected_type: Option<&schema::ExtendedType>,
        schema: &Schema,
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Vec<CompletionItem>;

    fn get_field_completions(
        &self,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        add_braces: bool,
        cursor_offset: usize,
    ) -> Vec<CompletionItem>;

    fn is_query_root(&self, ty: &schema::ExtendedType, schema: &Schema) -> bool;

    fn get_all_type_completions(&self, schema: &Schema) -> Vec<CompletionItem>;

    fn get_applicable_type_completions(
        &self,
        parent: &schema::ExtendedType,
        schema: &Schema,
        add_braces: bool,
        cursor_offset: usize,
    ) -> Vec<CompletionItem>;

    fn get_prefix_at_cursor(&self, cursor_offset: usize) -> (usize, usize);

    fn create_braced_snippet(
        &self,
        name: &str,
        cursor_offset: usize,
    ) -> Option<(String, InsertTextFormat, TextEdit)>;

    fn get_directive_completions(
        &self,
        schema: &Schema,
        location: ast::DirectiveLocation,
    ) -> Vec<CompletionItem>;

    fn is_after_at(&self, cursor_offset: usize) -> bool;

    fn is_after_dots(&self, cursor_offset: usize) -> bool;

    fn is_after_on(&self, cursor_offset: usize) -> bool;

    fn is_after_pipe(&self, cursor_offset: usize) -> bool;

    fn is_after_implements(&self, cursor_offset: usize) -> bool;

    fn is_after_directive_open_paren(&self, cursor_offset: usize) -> bool;

    fn is_after_equals(&self, cursor_offset: usize) -> bool;

    fn get_type_before_equals(&self, cursor_offset: usize) -> Option<ast::Type>;

    fn get_word_prefix_before_paren(&self, cursor_offset: usize) -> Option<String>;

    fn is_after_equals_in_variable(&self, cursor_offset: usize, node: &Node) -> bool;

    fn is_after_equals_in_argument(&self, cursor_offset: usize, node: &Node) -> bool;

    fn is_after_colon_in_selection(&self, cursor_offset: usize) -> bool;

    fn is_after_question_mark(&self, cursor_offset: usize) -> bool;

    fn is_operation_type_position(&self, cursor_offset: usize, node: &Node) -> bool;

    fn is_schema_definition_position(&self, cursor_offset: usize, node: &Node) -> bool;

    fn get_word_prefix_at_cursor(&self, cursor_offset: usize) -> Option<String>;

    fn is_name_char(&self, c: char) -> bool;

    fn get_operation_type_keyword_completions(&self) -> Vec<CompletionItem>;

    fn get_schema_definition_keyword_completions(&self, prefix: &str) -> Vec<CompletionItem>;

    fn get_alias_completions(
        &self,
        parent_type: Option<&str>,
        schema: &Schema,
    ) -> Vec<CompletionItem>;
    fn get_directive_argument_completions(
        &self,
        directive_name: &str,
        schema: &Schema,
    ) -> Vec<CompletionItem>;
    fn get_union_member_completions(&self, schema: &Schema) -> Vec<CompletionItem>;
    fn get_implements_interface_completions(&self, schema: &Schema) -> Vec<CompletionItem>;
    fn get_variable_default_completions(
        &self,
        expected_type: &ast::Type,
        schema: &Schema,
    ) -> Vec<CompletionItem>;

    fn parse_type_string(&self, text: &str) -> ast::Type;
}

impl DocumentCompletion for DocumentState {
    fn get_completion_items(
        &self,
        position: Position,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Vec<CompletionItem> {
        let byte_offset = self.position_to_byte(position);

        // Never provide GraphQL completions inside GraphQL comments.
        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();
            let tree_len = root.end_byte();
            let block_end = offset + tree_len;

            let allowed_end = if self.language.is_host_language() {
                block_end.saturating_add(1)
            } else {
                block_end
            };

            if byte_offset >= offset
                && byte_offset <= allowed_end
                && is_cursor_inside_comment(self, root, offset, byte_offset)
            {
                return Vec::new();
            }
        }

        // Check for keyword prefixes at the root level or in operation type positions
        if let Some(prefix) = self.get_word_prefix_at_cursor(byte_offset) {
            match prefix.as_str() {
                "qu" | "mu" | "su" | "que" | "mut" | "sub" => {
                    return self.get_operation_type_keyword_completions();
                }
                "ty" | "in" | "un" | "en" | "sc" | "ex" | "di" | "typ" | "int" | "uni" | "enu"
                | "sca" | "ext" | "dir" => {
                    return self.get_schema_definition_keyword_completions(&prefix);
                }
                _ => {}
            }
        }

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
                && let Some(items) = self.find_completions_in_tree(
                    root,
                    offset,
                    byte_offset,
                    schema,
                    fragments,
                    resolve_requirements.clone(),
                )
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
        resolve_requirements: FragmentRequirementsResolver,
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

        // New triggers
        if self.is_after_pipe(cursor_offset) {
            return Some(self.get_union_member_completions(schema));
        }

        if self.is_after_implements(cursor_offset) {
            return Some(self.get_implements_interface_completions(schema));
        }

        if self.is_after_directive_open_paren(cursor_offset) {
            let directive_name = self.get_word_prefix_before_paren(cursor_offset);
            if let Some(name) = directive_name {
                return Some(self.get_directive_argument_completions(&name, schema));
            }
        }

        if self.is_after_equals(cursor_offset) {
            if let Some(ast_type) = self.get_type_before_equals(cursor_offset) {
                return Some(self.get_variable_default_completions(&ast_type, schema));
            }

            let mut curr = start_node;
            while let Some(node) = curr {
                if matches!(
                    node.kind(),
                    "variable_definition" | "input_value_definition"
                ) {
                    let mut vd_cursor = node.walk();
                    let mut var_type_text = None;
                    for vd_child in node.children(&mut vd_cursor) {
                        if vd_child.kind() == "type" {
                            var_type_text = Some(self.get_node_text(vd_child, offset));
                            break;
                        }
                    }

                    if let Some(text) = var_type_text {
                        let ast_type = self.parse_type_string(&text);
                        return Some(self.get_variable_default_completions(&ast_type, schema));
                    }
                }
                curr = node.parent();
            }
        }

        // Handle inline fragment type completion after '... on '
        if self.is_after_on(cursor_offset) {
            let context_node = start_node
                .and_then(|n| {
                    self.find_ancestor_by_kinds(
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
                    resolve_requirements.clone(),
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

            // Field alias trigger
            if self.is_after_colon_in_selection(cursor_offset) && node.kind() == "field" {
                let parent_type = self.find_parent_type_for_node(node, offset, schema);
                return Some(self.get_alias_completions(
                    parent_type.as_ref().map(|t| t.name().as_str()),
                    schema,
                ));
            }

            // Try node-specific completions
            if let Some(items) = self.try_node_kind_completions(
                node,
                root,
                offset,
                cursor_offset,
                schema,
                fragments,
                resolve_requirements.clone(),
            ) {
                return Some(items);
            }

            curr = node.parent();
        }

        None
    }

    fn try_node_kind_completions(
        &self,
        current: Node,
        root: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Option<Vec<CompletionItem>> {
        match current.kind() {
            "operation_type" => Some(self.get_operation_type_keyword_completions()),
            "schema_document" | "schema_definition" | "document" => {
                let prefix = self
                    .get_word_prefix_at_cursor(cursor_offset)
                    .unwrap_or_default();
                Some(self.get_schema_definition_keyword_completions(&prefix))
            }
            "union_type_definition" => Some(self.get_union_member_completions(schema)),
            "implements" | "implements_interface" => {
                Some(self.get_implements_interface_completions(schema))
            }
            "directive" => {
                let name_node = self.find_child_by_kind(current, "name");
                if let Some(name_node) = name_node {
                    let dir_name = self.get_node_text(name_node, offset);
                    return Some(self.get_directive_argument_completions(&dir_name, schema));
                }
                None
            }
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
            "variable"
            | "variable_definitions"
            | "variable_definition"
            | "input_value_definition" => {
                if self.is_after_equals(cursor_offset) {
                    let vd_node = if matches!(
                        current.kind(),
                        "variable_definition" | "input_value_definition"
                    ) {
                        Some(current)
                    } else {
                        self.find_ancestor_by_kinds(
                            current,
                            &["variable_definition", "input_value_definition"],
                        )
                    };

                    if let Some(vd) = vd_node {
                        let mut vd_cursor = vd.walk();
                        let mut var_type_text = None;
                        for vd_child in vd.children(&mut vd_cursor) {
                            if vd_child.kind() == "type" {
                                var_type_text = Some(self.get_node_text(vd_child, offset));
                                break;
                            }
                        }

                        if let Some(text) = var_type_text {
                            let ast_type = self.parse_type_string(&text);
                            return Some(self.get_variable_default_completions(&ast_type, schema));
                        }
                    }
                }

                if matches!(
                    current.kind(),
                    "variable" | "variable_definitions" | "variable_definition"
                ) {
                    return Some(self.get_operation_variables(root, offset, cursor_offset));
                }
                None
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

                if let Some(field_node) = field_node {
                    // Check if we are at a value position
                    if let Some(expected_ast_type) = self.find_expected_ast_type_for_node(
                        current,
                        offset,
                        Some(cursor_offset),
                        schema,
                    ) {
                        let mut value_items =
                            self.get_variable_default_completions(&expected_ast_type, schema);
                        items.append(&mut value_items);

                        // If it's an InputObject, we still want to suggest its fields
                        if let Some(expected_type) = schema
                            .types
                            .get(expected_ast_type.inner_named_type().as_str())
                            && let schema::ExtendedType::InputObject(input_obj) = expected_type
                        {
                            for (name, def) in &input_obj.fields {
                                items.push(CompletionItem {
                                    label: name.to_string(),
                                    kind: Some(CompletionItemKind::FIELD),
                                    detail: Some(def.ty.to_string()),
                                    documentation: def.description.as_ref().map(|d| {
                                        lsp_types::Documentation::MarkupContent(
                                            lsp_types::MarkupContent {
                                                kind: lsp_types::MarkupKind::Markdown,
                                                value: d.to_string(),
                                            },
                                        )
                                    }),
                                    ..Default::default()
                                });
                            }
                        }
                        return Some(items);
                    }

                    if let Some(parent_type) =
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
                                for arg in &fdef.arguments {
                                    items.push(CompletionItem {
                                        label: arg.name.to_string(),
                                        kind: Some(CompletionItemKind::FIELD),
                                        detail: Some(arg.ty.to_string()),
                                        documentation: arg.description.as_ref().map(|d| {
                                            lsp_types::Documentation::MarkupContent(
                                                lsp_types::MarkupContent {
                                                    kind: lsp_types::MarkupKind::Markdown,
                                                    value: d.to_string(),
                                                },
                                            )
                                        }),
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                    }
                } else if let Some(directive_node) =
                    self.find_ancestor_by_kind(current, "directive")
                {
                    // Check if we are at a value position
                    if let Some(expected_ast_type) = self.find_expected_ast_type_for_node(
                        current,
                        offset,
                        Some(cursor_offset),
                        schema,
                    ) {
                        let mut value_items =
                            self.get_variable_default_completions(&expected_ast_type, schema);
                        items.append(&mut value_items);
                        return Some(items);
                    }

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
                                lsp_types::Documentation::MarkupContent(lsp_types::MarkupContent {
                                    kind: lsp_types::MarkupKind::Markdown,
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
                                lsp_types::Documentation::MarkupContent(lsp_types::MarkupContent {
                                    kind: lsp_types::MarkupKind::Markdown,
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
                Some(self.get_fragment_name_completions(
                    fragments,
                    parent_type.as_ref(),
                    schema,
                    resolve_requirements,
                ))
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
                    resolve_requirements,
                )
            }
            "selection_set" | "operation_definition" => self.complete_selection_set_at_node(
                current,
                offset,
                cursor_offset,
                schema,
                fragments,
                resolve_requirements,
            ),
            _ => None,
        }
    }

    fn try_directive_completions(
        &self,
        current: Node,
        offset: usize,
        schema: &Schema,
    ) -> Option<Vec<CompletionItem>> {
        directives::try_directive_completions(self, current, offset, schema)
    }

    fn find_directive_context_node<'a>(
        &self,
        current: Node<'a>,
        offset: usize,
    ) -> Option<Node<'a>> {
        directives::find_directive_context_node(self, current, offset)
    }

    fn find_directive_location<'a>(
        &self,
        p: Node<'a>,
        offset: usize,
    ) -> Option<ast::DirectiveLocation> {
        directives::find_directive_location(self, p, offset)
    }

    fn get_operation_directive_location(
        &self,
        node: Node,
        offset: usize,
    ) -> ast::DirectiveLocation {
        directives::get_operation_directive_location(self, node, offset)
    }

    fn find_preceding_field_type_internal(
        &self,
        selection_set: Node,
        offset: usize,
        cursor_offset: usize,
        current_type: &schema::ExtendedType,
        schema: &Schema,
    ) -> Option<schema::ExtendedType> {
        fields::find_preceding_field_type_internal(
            self,
            selection_set,
            offset,
            cursor_offset,
            current_type,
            schema,
        )
    }

    fn has_trailing_selection_set_internal(&self, cursor_offset: usize) -> bool {
        fields::has_trailing_selection_set_internal(self, cursor_offset)
    }

    fn find_field_node_before_offset<'a>(
        &self,
        selection_set: Node<'a>,
        offset: usize,
        cursor_offset: usize,
    ) -> Option<Node<'a>> {
        fields::find_field_node_before_offset(self, selection_set, offset, cursor_offset)
    }

    fn find_expected_type_for_node(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: Option<usize>,
        schema: &Schema,
    ) -> Option<schema::ExtendedType> {
        crate::shared::type_resolver::find_expected_type_for_node(
            self,
            node,
            offset,
            cursor_offset,
            schema,
        )
    }

    fn find_expected_ast_type_for_node(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: Option<usize>,
        schema: &Schema,
    ) -> Option<ast::Type> {
        crate::shared::type_resolver::find_expected_ast_type_for_node(
            self,
            node,
            offset,
            cursor_offset,
            schema,
        )
    }

    fn complete_selection_set_at_node(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Option<Vec<CompletionItem>> {
        fields::complete_selection_set_at_node(
            self,
            node,
            offset,
            cursor_offset,
            schema,
            fragments,
            resolve_requirements,
        )
    }

    fn get_operation_variables(
        &self,
        root: Node,
        offset: usize,
        cursor_offset: usize,
    ) -> Vec<CompletionItem> {
        operations::get_operation_variables(self, root, offset, cursor_offset)
    }

    fn complete_operation(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Option<Vec<CompletionItem>> {
        operations::complete_operation(
            self,
            node,
            offset,
            cursor_offset,
            schema,
            fragments,
            resolve_requirements,
        )
    }

    fn complete_fragment(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Option<Vec<CompletionItem>> {
        fragments::complete_fragment(
            self,
            node,
            offset,
            cursor_offset,
            schema,
            fragments,
            resolve_requirements,
        )
    }

    fn complete_inline_fragment(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Option<Vec<CompletionItem>> {
        fragments::complete_inline_fragment(
            self,
            node,
            offset,
            cursor_offset,
            schema,
            fragments,
            resolve_requirements,
        )
    }

    fn complete_selection_set_recursive(
        &self,
        node: Node,
        offset: usize,
        cursor_offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Option<Vec<CompletionItem>> {
        fields::complete_selection_set_recursive(
            self,
            node,
            offset,
            cursor_offset,
            parent_type,
            schema,
            fragments,
            resolve_requirements,
        )
    }

    fn complete_field(
        &self,
        field_node: Node,
        offset: usize,
        cursor_offset: usize,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        fragments: &[FragmentCompletionInfo],
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Option<Vec<CompletionItem>> {
        fields::complete_field(
            self,
            field_node,
            offset,
            cursor_offset,
            parent_type,
            schema,
            fragments,
            resolve_requirements,
        )
    }

    fn get_fragment_name_completions(
        &self,
        fragments: &[FragmentCompletionInfo],
        expected_type: Option<&schema::ExtendedType>,
        schema: &Schema,
        resolve_requirements: FragmentRequirementsResolver,
    ) -> Vec<CompletionItem> {
        fragments::get_fragment_name_completions(
            self,
            fragments,
            expected_type,
            schema,
            resolve_requirements,
        )
    }

    fn get_field_completions(
        &self,
        parent_type: &schema::ExtendedType,
        schema: &Schema,
        add_braces: bool,
        cursor_offset: usize,
    ) -> Vec<CompletionItem> {
        fields::get_field_completions(self, parent_type, schema, add_braces, cursor_offset)
    }

    fn is_query_root(&self, ty: &schema::ExtendedType, schema: &Schema) -> bool {
        // This helper is simple enough to duplicate or just implement here using DocumentState which we have self.
        // But wait, is_query_root was implemented in DocumentCompletion using schema. It doesn't use self.
        schema
            .root_operation(ast::OperationType::Query)
            .and_then(|root_name| schema.types.get(root_name.as_str()))
            .map(|root_type| root_type.name() == ty.name())
            .unwrap_or(false)
    }

    fn get_all_type_completions(&self, schema: &Schema) -> Vec<CompletionItem> {
        values::get_all_type_completions(schema)
    }

    fn get_applicable_type_completions(
        &self,
        parent: &schema::ExtendedType,
        schema: &Schema,
        add_braces: bool,
        cursor_offset: usize,
    ) -> Vec<CompletionItem> {
        values::get_applicable_type_completions(self, parent, schema, add_braces, cursor_offset)
    }

    fn get_prefix_at_cursor(&self, cursor_offset: usize) -> (usize, usize) {
        cursor::get_prefix_at_cursor(self, cursor_offset)
    }

    fn create_braced_snippet(
        &self,
        name: &str,
        cursor_offset: usize,
    ) -> Option<(String, InsertTextFormat, TextEdit)> {
        utils::create_braced_snippet(self, name, cursor_offset)
    }

    fn get_directive_completions(
        &self,
        schema: &Schema,
        location: ast::DirectiveLocation,
    ) -> Vec<CompletionItem> {
        directives::get_directive_completions(self, schema, location)
    }

    fn is_after_at(&self, cursor_offset: usize) -> bool {
        cursor::is_after_at(self, cursor_offset)
    }

    fn is_after_dots(&self, cursor_offset: usize) -> bool {
        cursor::is_after_dots(self, cursor_offset)
    }

    fn is_after_on(&self, cursor_offset: usize) -> bool {
        cursor::is_after_on(self, cursor_offset)
    }

    fn is_after_pipe(&self, cursor_offset: usize) -> bool {
        cursor::is_after_pipe(self, cursor_offset)
    }

    fn is_after_implements(&self, cursor_offset: usize) -> bool {
        cursor::is_after_implements(self, cursor_offset)
    }

    fn is_after_directive_open_paren(&self, cursor_offset: usize) -> bool {
        cursor::is_after_directive_open_paren(self, cursor_offset)
    }

    fn is_after_equals(&self, cursor_offset: usize) -> bool {
        cursor::is_after_equals(self, cursor_offset)
    }

    fn get_type_before_equals(&self, cursor_offset: usize) -> Option<ast::Type> {
        utils::get_type_before_equals(self, cursor_offset)
    }

    fn get_word_prefix_before_paren(&self, cursor_offset: usize) -> Option<String> {
        cursor::get_word_prefix_before_paren(self, cursor_offset)
    }

    fn is_after_equals_in_variable(&self, cursor_offset: usize, node: &Node) -> bool {
        cursor::is_after_equals_in_variable(self, cursor_offset, node)
    }

    fn is_after_equals_in_argument(&self, cursor_offset: usize, node: &Node) -> bool {
        cursor::is_after_equals_in_argument(self, cursor_offset, node)
    }

    fn is_after_colon_in_selection(&self, cursor_offset: usize) -> bool {
        cursor::is_after_colon_in_selection(self, cursor_offset)
    }

    fn is_after_question_mark(&self, cursor_offset: usize) -> bool {
        cursor::is_after_question_mark(self, cursor_offset)
    }

    fn is_operation_type_position(&self, cursor_offset: usize, node: &Node) -> bool {
        cursor::is_operation_type_position(self, cursor_offset, node)
    }

    fn is_schema_definition_position(&self, cursor_offset: usize, node: &Node) -> bool {
        cursor::is_schema_definition_position(self, cursor_offset, node)
    }

    fn get_word_prefix_at_cursor(&self, cursor_offset: usize) -> Option<String> {
        cursor::get_word_prefix_at_cursor(self, cursor_offset)
    }

    fn is_name_char(&self, c: char) -> bool {
        cursor::is_name_char(c)
    }

    fn get_operation_type_keyword_completions(&self) -> Vec<CompletionItem> {
        keywords::get_operation_type_keyword_completions()
    }

    fn get_schema_definition_keyword_completions(&self, prefix: &str) -> Vec<CompletionItem> {
        keywords::get_schema_definition_keyword_completions(prefix)
    }

    fn get_alias_completions(
        &self,
        parent_type: Option<&str>,
        schema: &Schema,
    ) -> Vec<CompletionItem> {
        fields::get_alias_completions(self, parent_type, schema)
    }

    fn get_directive_argument_completions(
        &self,
        directive_name: &str,
        schema: &Schema,
    ) -> Vec<CompletionItem> {
        directives::get_directive_argument_completions(self, directive_name, schema)
    }

    fn get_union_member_completions(&self, schema: &Schema) -> Vec<CompletionItem> {
        values::get_union_member_completions(schema)
    }

    fn get_implements_interface_completions(&self, schema: &Schema) -> Vec<CompletionItem> {
        values::get_implements_interface_completions(schema)
    }

    fn get_variable_default_completions(
        &self,
        expected_type: &ast::Type,
        schema: &Schema,
    ) -> Vec<CompletionItem> {
        values::get_variable_default_completions(self, expected_type, schema)
    }

    fn parse_type_string(&self, text: &str) -> ast::Type {
        crate::shared::type_resolver::parse_type_string(text)
    }
}
