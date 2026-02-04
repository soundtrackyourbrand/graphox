use crate::document::DocumentState;
use apollo_compiler::{Schema, schema};
use tower_lsp::lsp_types::*;
use tree_sitter::Node;

#[derive(Clone)]
pub struct FragmentCompletionInfo {
    pub name: String,
    pub type_condition: String,
    pub description: Option<String>,
    pub import_path: Option<String>,
    pub is_public: bool,
    pub uri: Url,
    pub package_root: Option<std::path::PathBuf>,
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

            if byte_offset >= offset
                && byte_offset <= offset + tree_len
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
        let local_byte = cursor_offset.saturating_sub(offset);

        let mut node = root.descendant_for_byte_range(local_byte.saturating_sub(1), local_byte);

        while let Some(current) = node {
            match current.kind() {
                "selection_set" | "operation_definition" | "fragment_definition" | "inline_fragment" => {
                    // Check if we are right after dots
                    if self.is_after_dots(offset, local_byte) {
                         if let Some(items) = self.complete_selection_set_at_node(
                            current,
                            offset,
                            cursor_offset,
                            schema,
                            fragments,
                        ) {
                            // Filter these items to ONLY include fragments
                            return Some(items.into_iter().filter(|i| i.kind == Some(CompletionItemKind::SNIPPET)).collect());
                        }
                    }
                }
                _ => {}
            }
            
            match current.kind() {
                "type_condition" | "named_type" => {
                    return Some(self.get_all_type_completions(schema));
                }
                "variable" | "variable_definitions" | "arguments" => {
                    return Some(self.get_operation_variables(root, offset, cursor_offset));
                }
                "fragment_spread" => {
                    let parent_type = self.find_parent_type_for_node(current, offset, schema);
                    return Some(self.get_fragment_name_completions(fragments, parent_type.as_ref(), schema));
                }
                "fragment_definition" => {
                    if self.is_after_on(offset, local_byte) {
                        return Some(self.get_all_type_completions(schema));
                    }
                    if let Some(items) = self.complete_selection_set_at_node(
                        current,
                        offset,
                        cursor_offset,
                        schema,
                        fragments,
                    ) {
                        return Some(items);
                    }
                }
                "selection_set" | "operation_definition" => {
                    if let Some(items) = self.complete_selection_set_at_node(
                        current,
                        offset,
                        cursor_offset,
                        schema,
                        fragments,
                    ) {
                        return Some(items);
                    }
                }
                _ => {}
            }
            node = current.parent();
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
                    return self.complete_selection_set_at_node(
                        parent,
                        offset,
                        cursor_offset,
                        schema,
                        fragments,
                    );
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
        let mut current = root.descendant_for_byte_range(local_byte.saturating_sub(1), local_byte);
        let mut target_op = None;

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
        fragments: &[FragmentCompletionInfo],
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
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "type_condition" {
                let range = (child.start_byte() + offset)..(child.end_byte() + offset);
                if cursor_offset >= range.start && cursor_offset <= range.end {
                    return Some(self.get_all_type_completions(schema));
                }
            } else if child.kind() == "selection_set" {
                let range = (child.start_byte() + offset)..(child.end_byte() + offset);
                if cursor_offset >= range.start
                    && cursor_offset <= range.end
                    && let Some(type_name) = self.get_fragment_type_condition(node, offset)
                    && let Some(type_def) = schema.types.get(type_name.as_str())
                {
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
            // If no type condition, it inherits parent's type
            let mut current = node.parent()?;
            while current.kind() != "selection_set" {
                current = current.parent()?;
            }
            self.find_parent_type_for_node(node, offset, schema)
        };

        if let Some(type_def) = parent_type {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "selection_set" {
                    return self.complete_selection_set_recursive(
                        child,
                        offset,
                        cursor_offset,
                        &type_def,
                        schema,
                        fragments,
                    );
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
        fragments: &[FragmentCompletionInfo],
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
            found?
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
                        } else if inner_child.kind() == "fragment_spread" || inner_child.kind() == "..." {
                            return Some(self.get_fragment_name_completions(fragments, Some(parent_type), schema));
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
                    return Some(self.get_fragment_name_completions(fragments, Some(parent_type), schema));
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
        fragments: &[FragmentCompletionInfo],
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
                            if obj.implements_interfaces.iter().any(|i| i.as_str() == f.type_condition) {
                                return true;
                            }
                        }
                        schema::ExtendedType::Interface(iface) => {
                            if iface.implements_interfaces.iter().any(|i| i.as_str() == f.type_condition) {
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
                    
                    // Also check if the current type is a member of the fragment's type (if fragment is a union)
                    if let Some(frag_type) = schema.types.get(f.type_condition.as_str()) {
                        match frag_type {
                            schema::ExtendedType::Union(u) => {
                                if u.members.iter().any(|m| m.as_str() == parent_name.as_str()) {
                                    return true;
                                }
                            }
                            schema::ExtendedType::Interface(_) => {
                                // If the fragment is on an interface, and our parent type implements it
                                // We already handled this above for Object/Interface parents.
                            }
                            _ => {}
                        }
                    }

                    false
                } else {
                    true
                }
            })
            .map(|f| {
                let mut documentation = f.description.clone().unwrap_or_default();
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

    fn get_field_completions(&self, parent_type: &schema::ExtendedType) -> Vec<CompletionItem> {
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
        items
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

    fn is_after_dots(&self, offset: usize, local_byte: usize) -> bool {
        let start = offset + local_byte.saturating_sub(10);
        let end = offset + local_byte;
        let slice = self.rope.byte_slice(start..end).to_string();
        let mut dot_count = 0;
        for c in slice.chars().rev() {
            if c.is_whitespace() {
                continue;
            }
            if c == '.' {
                dot_count += 1;
                if dot_count == 3 {
                    return true;
                }
            } else {
                break;
            }
        }
        false
    }

    fn is_after_on(&self, offset: usize, local_byte: usize) -> bool {
        let start = offset + local_byte.saturating_sub(10);
        let end = offset + local_byte;
        let slice = self.rope.byte_slice(start..end).to_string();
        let mut found_n = false;
        for c in slice.chars().rev() {
            if c.is_whitespace() {
                continue;
            }
            if !found_n {
                if c == 'n' || c == 'N' {
                    found_n = true;
                } else {
                    return false;
                }
            } else if c == 'o' || c == 'O' {
                return true;
            } else {
                return false;
            }
        }
        false
    }
}
