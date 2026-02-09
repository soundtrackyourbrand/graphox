use apollo_compiler::{Schema, ast, schema};
use graphox_core::document::DocumentState;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionTextEdit, Documentation, MarkupContent,
    MarkupKind,
};
use tree_sitter::Node;

use crate::completion::types::FragmentCompletionInfo;
use crate::completion::{fragments, operations, utils};

pub fn find_preceding_field_type_internal(
    doc: &DocumentState,
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
            doc.find_child_by_kind(child, "field")
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
        && let Some(name_node) = doc.extract_field_components(field).name
    {
        let field_name = doc.get_node_text(name_node, offset);
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

pub fn has_trailing_selection_set_internal(doc: &DocumentState, cursor_offset: usize) -> bool {
    let remaining = doc.rope.byte_slice(cursor_offset..).to_string();
    for c in remaining.chars() {
        if c.is_whitespace() {
            continue;
        }
        return c == '{';
    }
    false
}

pub fn find_field_node_before_offset<'a>(
    doc: &DocumentState,
    selection_set: Node<'a>,
    offset: usize,
    cursor_offset: usize,
) -> Option<Node<'a>> {
    let mut cursor = selection_set.walk();
    let mut last_field = None;
    for child in selection_set.children(&mut cursor) {
        let field_node = if child.kind() == "selection" {
            doc.find_child_by_kind(child, "field")
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

pub fn complete_selection_set_at_node(
    doc: &DocumentState,
    node: Node,
    offset: usize,
    cursor_offset: usize,
    schema: &Schema,
    fragments: &[FragmentCompletionInfo],
) -> Option<Vec<CompletionItem>> {
    match node.kind() {
        "operation_definition" => {
            operations::complete_operation(doc, node, offset, cursor_offset, schema, fragments)
        }
        "fragment_definition" => {
            fragments::complete_fragment(doc, node, offset, cursor_offset, schema, fragments)
        }
        "inline_fragment" => {
            fragments::complete_inline_fragment(doc, node, offset, cursor_offset, schema, fragments)
        }
        "selection_set" => {
            if let Some(parent) = node.parent() {
                match parent.kind() {
                    "operation_definition" => {
                        return operations::complete_operation(
                            doc,
                            parent,
                            offset,
                            cursor_offset,
                            schema,
                            fragments,
                        );
                    }
                    "fragment_definition" => {
                        return fragments::complete_fragment(
                            doc,
                            parent,
                            offset,
                            cursor_offset,
                            schema,
                            fragments,
                        );
                    }
                    "inline_fragment" => {
                        return fragments::complete_inline_fragment(
                            doc,
                            parent,
                            offset,
                            cursor_offset,
                            schema,
                            fragments,
                        );
                    }
                    "field" => {
                        if let Some(containing_type) =
                            doc.find_parent_type_for_node(parent, offset, schema)
                            && let Some(field_name_node) = doc.extract_field_components(parent).name
                        {
                            let field_name = doc.get_node_text(field_name_node, offset);
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
                                return complete_selection_set_recursive(
                                    doc,
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

pub fn complete_selection_set_recursive(
    doc: &DocumentState,
    node: Node,
    offset: usize,
    cursor_offset: usize,
    parent_type: &schema::ExtendedType,
    schema: &Schema,
    fragments_info: &[FragmentCompletionInfo],
) -> Option<Vec<CompletionItem>> {
    let target_node = if node.kind() == "selection_set" {
        node
    } else {
        doc.find_child_by_kind(node, "selection_set")?
    };

    if !doc.is_cursor_in_node_range(target_node, offset, cursor_offset) {
        return None;
    }

    let mut cursor = target_node.walk();
    for child in target_node.children(&mut cursor) {
        if doc.is_cursor_in_node_range(child, offset, cursor_offset) {
            let kind = child.kind();
            if kind == "selection" {
                let mut inner = child.walk();
                for inner_child in child.children(&mut inner) {
                    if inner_child.kind() == "field" {
                        if let Some(items) = complete_field(
                            doc,
                            inner_child,
                            offset,
                            cursor_offset,
                            parent_type,
                            schema,
                            fragments_info,
                        ) {
                            return Some(items);
                        }
                    } else if inner_child.kind() == "fragment_spread" || inner_child.kind() == "..."
                    {
                        return Some(fragments::get_fragment_name_completions(
                            doc,
                            fragments_info,
                            Some(parent_type),
                            schema,
                        ));
                    }
                }
            } else if kind == "field" {
                if let Some(items) = complete_field(
                    doc,
                    child,
                    offset,
                    cursor_offset,
                    parent_type,
                    schema,
                    fragments_info,
                ) {
                    return Some(items);
                }
            } else if kind == "fragment_spread" || kind == "..." {
                return Some(fragments::get_fragment_name_completions(
                    doc,
                    fragments_info,
                    Some(parent_type),
                    schema,
                ));
            }
        }
    }

    let has_selection_set = has_trailing_selection_set_internal(doc, cursor_offset);
    Some(get_field_completions(
        doc,
        parent_type,
        schema,
        !has_selection_set,
        cursor_offset,
    ))
}

pub fn complete_field(
    doc: &DocumentState,
    field_node: Node,
    offset: usize,
    cursor_offset: usize,
    parent_type: &schema::ExtendedType,
    schema: &Schema,
    fragments: &[FragmentCompletionInfo],
) -> Option<Vec<CompletionItem>> {
    let components = doc.extract_field_components(field_node);

    if let Some(field_name_node) = components.name {
        let field_name = doc.get_node_text(field_name_node, offset);

        let field_def = match parent_type {
            schema::ExtendedType::Object(obj) => obj.fields.get(field_name.as_str()),
            schema::ExtendedType::Interface(iface) => iface.fields.get(field_name.as_str()),
            _ => None,
        };

        if let Some(field_def) = field_def {
            if let Some(args) = components.arguments
                && doc.is_cursor_in_node_range(args, offset, cursor_offset)
            {
                return Some(operations::get_operation_variables(
                    doc,
                    field_node,
                    offset,
                    cursor_offset,
                ));
            }

            if let Some(sss) = components.selection_set
                && doc.is_cursor_in_node_range(sss, offset, cursor_offset)
            {
                let field_type_name = field_def.ty.inner_named_type();
                if let Some(field_type_def) = schema.types.get(field_type_name.as_str()) {
                    return complete_selection_set_recursive(
                        doc,
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

pub fn get_field_completions(
    doc: &DocumentState,
    parent_type: &schema::ExtendedType,
    schema: &Schema,
    add_braces: bool,
    cursor_offset: usize,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    match parent_type {
        schema::ExtendedType::Object(obj) => {
            for (name, def) in &obj.fields {
                let mut item = CompletionItem {
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
                };

                let field_type_name = def.ty.inner_named_type();
                let returns_object_or_interface = matches!(
                    schema.types.get(field_type_name.as_str()),
                    Some(schema::ExtendedType::Object(_))
                        | Some(schema::ExtendedType::Interface(_))
                );

                if add_braces
                    && returns_object_or_interface
                    && let Some((snippet, format, text_edit)) =
                        utils::create_braced_snippet(doc, name, cursor_offset)
                {
                    item.insert_text = Some(snippet);
                    item.insert_text_format = Some(format);
                    item.text_edit = Some(CompletionTextEdit::Edit(text_edit));
                }

                items.push(item);
            }
        }
        schema::ExtendedType::Interface(iface) => {
            for (name, def) in &iface.fields {
                let mut item = CompletionItem {
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
                };

                let field_type_name = def.ty.inner_named_type();
                let returns_object_or_interface = matches!(
                    schema.types.get(field_type_name.as_str()),
                    Some(schema::ExtendedType::Object(_))
                        | Some(schema::ExtendedType::Interface(_))
                );

                if add_braces
                    && returns_object_or_interface
                    && let Some((snippet, format, text_edit)) =
                        utils::create_braced_snippet(doc, name, cursor_offset)
                {
                    item.insert_text = Some(snippet);
                    item.insert_text_format = Some(format);
                    item.text_edit = Some(CompletionTextEdit::Edit(text_edit));
                }

                items.push(item);
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
    if is_query_root(parent_type, schema) {
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

/// Get completions for field aliases (after ':' in selections)
pub fn get_alias_completions(
    _doc: &DocumentState,
    parent_type: Option<&str>,
    schema: &Schema,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    if let Some(type_name) = parent_type
        && let Some(schema::ExtendedType::Object(obj)) = schema.types.get(type_name)
    {
        for name in obj.fields.keys() {
            // Suggest field names as potential aliases
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::FIELD),
                detail: Some(format!("Alias for field '{}'", name)),
                ..Default::default()
            });
        }
    }

    items
}
