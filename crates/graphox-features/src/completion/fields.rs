use apollo_compiler::{Schema, schema};
use graphox_core::document::DocumentState;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionTextEdit, Documentation, MarkupContent,
    MarkupKind,
};
use tree_sitter::Node;

use crate::completion::types::{FragmentCompletionInfo, FragmentRequirementsResolver};
use crate::completion::{fragments, operations, utils};
use crate::shared::{markdown_utils, schema_utils};

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
        let field_def = schema_utils::get_field_def(current_type, field_name.as_str());
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

#[allow(clippy::too_many_arguments)]
pub fn complete_selection_set_at_node(
    doc: &DocumentState,
    node: Node,
    offset: usize,
    cursor_offset: usize,
    schema: &Schema,
    subgraphs: Option<&[graphox_core::schema::SubgraphInfo]>,
    fragments: &[FragmentCompletionInfo],
    resolve_requirements: FragmentRequirementsResolver,
) -> Option<Vec<CompletionItem>> {
    match node.kind() {
        "operation_definition" => operations::complete_operation(
            doc,
            node,
            offset,
            cursor_offset,
            schema,
            subgraphs,
            fragments,
            resolve_requirements.clone(),
        ),
        "fragment_definition" => fragments::complete_fragment(
            doc,
            node,
            offset,
            cursor_offset,
            schema,
            subgraphs,
            fragments,
            resolve_requirements.clone(),
        ),
        "inline_fragment" => fragments::complete_inline_fragment(
            doc,
            node,
            offset,
            cursor_offset,
            schema,
            subgraphs,
            fragments,
            resolve_requirements.clone(),
        ),
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
                            subgraphs,
                            fragments,
                            resolve_requirements.clone(),
                        );
                    }
                    "fragment_definition" => {
                        return fragments::complete_fragment(
                            doc,
                            parent,
                            offset,
                            cursor_offset,
                            schema,
                            subgraphs,
                            fragments,
                            resolve_requirements.clone(),
                        );
                    }
                    "inline_fragment" => {
                        return fragments::complete_inline_fragment(
                            doc,
                            parent,
                            offset,
                            cursor_offset,
                            schema,
                            subgraphs,
                            fragments,
                            resolve_requirements.clone(),
                        );
                    }
                    "field" => {
                        if let Some(containing_type) =
                            doc.find_parent_type_for_node(parent, offset, schema)
                            && let Some(field_name_node) = doc.extract_field_components(parent).name
                        {
                            let field_name = doc.get_node_text(field_name_node, offset);
                            let field_def =
                                schema_utils::get_field_def(&containing_type, field_name.as_str());

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
                                    subgraphs,
                                    fragments,
                                    resolve_requirements.clone(),
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

#[allow(clippy::too_many_arguments)]
pub fn complete_selection_set_recursive(
    doc: &DocumentState,
    node: Node,
    offset: usize,
    cursor_offset: usize,
    parent_type: &schema::ExtendedType,
    schema: &Schema,
    subgraphs: Option<&[graphox_core::schema::SubgraphInfo]>,
    fragments_info: &[FragmentCompletionInfo],
    resolve_requirements: FragmentRequirementsResolver,
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
                            subgraphs,
                            fragments_info,
                            resolve_requirements.clone(),
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
                            resolve_requirements.clone(),
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
                    subgraphs,
                    fragments_info,
                    resolve_requirements.clone(),
                ) {
                    return Some(items);
                }
            } else if kind == "fragment_spread" || kind == "..." {
                return Some(fragments::get_fragment_name_completions(
                    doc,
                    fragments_info,
                    Some(parent_type),
                    schema,
                    resolve_requirements.clone(),
                ));
            }
        }
    }

    let has_selection_set = has_trailing_selection_set_internal(doc, cursor_offset);
    Some(get_field_completions(
        doc,
        parent_type,
        schema,
        subgraphs,
        !has_selection_set,
        cursor_offset,
    ))
}

#[allow(clippy::too_many_arguments)]
pub fn complete_field(
    doc: &DocumentState,
    field_node: Node,
    offset: usize,
    cursor_offset: usize,
    parent_type: &schema::ExtendedType,
    schema: &Schema,
    subgraphs: Option<&[graphox_core::schema::SubgraphInfo]>,
    fragments: &[FragmentCompletionInfo],
    resolve_requirements: FragmentRequirementsResolver,
) -> Option<Vec<CompletionItem>> {
    let components = doc.extract_field_components(field_node);

    if let Some(field_name_node) = components.name {
        let field_name = doc.get_node_text(field_name_node, offset);

        let field_def = schema_utils::get_field_def(parent_type, field_name.as_str());

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
                        subgraphs,
                        fragments,
                        resolve_requirements,
                    );
                }
            }
        }
    }
    None
}

fn build_subgraph_field_info(
    subgraphs: &[graphox_core::schema::SubgraphInfo],
    type_name: &str,
    field_name: &str,
) -> Vec<String> {
    let mut found_subgraphs = Vec::new();
    for sg in subgraphs {
        if let Some(sg_ty) = sg.schema.types.get(type_name) {
            let has_field = match sg_ty {
                schema::ExtendedType::Object(obj) => obj.fields.contains_key(field_name),
                schema::ExtendedType::Interface(iface) => iface.fields.contains_key(field_name),
                _ => false,
            };

            if has_field {
                let mut sg_info = sg.name.clone();
                if let Some(owner) = &sg.owner {
                    sg_info.push_str(" (");
                    sg_info.push_str(owner);
                    sg_info.push(')');
                }

                // Add SLO info if available
                let slo = sg
                    .field_slos
                    .get(type_name)
                    .and_then(|type_slos| type_slos.get(field_name).copied())
                    .or(sg.schema_slo);

                if let Some(slo) = slo {
                    sg_info.push_str(" [SLO: ");
                    sg_info.push_str(slo.as_str());
                    sg_info.push(']');
                }

                found_subgraphs.push(sg_info);
            }
        }
    }
    found_subgraphs
}

pub fn get_field_completions(
    doc: &DocumentState,
    parent_type: &schema::ExtendedType,
    schema: &Schema,
    subgraphs: Option<&[graphox_core::schema::SubgraphInfo]>,
    add_braces: bool,
    cursor_offset: usize,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    match parent_type {
        schema::ExtendedType::Object(obj) => {
            for (name, def) in &obj.fields {
                let mut detail = Some(def.ty.to_string());
                let mut documentation_value = markdown_utils::describe_field_markdown(
                    obj.name.as_str(),
                    name,
                    def.ty.to_string().as_str(),
                    def.description.as_deref(),
                );

                if let Some(subgraphs) = subgraphs {
                    let found_subgraphs =
                        build_subgraph_field_info(subgraphs, obj.name.as_str(), name);
                    if !found_subgraphs.is_empty() {
                        let subgraphs_str = found_subgraphs.join(", ");
                        detail = Some(format!("{} [{}]", def.ty, subgraphs_str));
                        documentation_value.push_str("\n\n**Subgraphs:** ");
                        documentation_value.push_str(&subgraphs_str);
                    }
                }

                let mut item = CompletionItem {
                    label: name.to_string(),
                    kind: Some(CompletionItemKind::FIELD),
                    detail,
                    documentation: Some(Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: documentation_value,
                    })),
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
                let mut detail = Some(def.ty.to_string());
                let mut documentation_value = markdown_utils::describe_field_markdown(
                    iface.name.as_str(),
                    name,
                    def.ty.to_string().as_str(),
                    def.description.as_deref(),
                );

                if let Some(subgraphs) = subgraphs {
                    let found_subgraphs =
                        build_subgraph_field_info(subgraphs, iface.name.as_str(), name);
                    if !found_subgraphs.is_empty() {
                        let subgraphs_str = found_subgraphs.join(", ");
                        detail = Some(format!("{} [{}]", def.ty, subgraphs_str));
                        documentation_value.push_str("\n\n**Subgraphs:** ");
                        documentation_value.push_str(&subgraphs_str);
                    }
                }

                let mut item = CompletionItem {
                    label: name.to_string(),
                    kind: Some(CompletionItemKind::FIELD),
                    detail,
                    documentation: Some(Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: documentation_value,
                    })),
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
        documentation: Some(Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: markdown_utils::describe_field_markdown(
                parent_type.name(),
                "__typename",
                "String!",
                Some("The GraphQL type name of the current selection."),
            ),
        })),
        ..Default::default()
    });
    if schema_utils::is_query_root(parent_type, schema) {
        items.push(CompletionItem {
            label: "__schema".to_string(),
            kind: Some(CompletionItemKind::FIELD),
            detail: Some("__Schema!".to_string()),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: markdown_utils::describe_field_markdown(
                    parent_type.name(),
                    "__schema",
                    "__Schema!",
                    Some("Access the current schema introspection object."),
                ),
            })),
            ..Default::default()
        });
        items.push(CompletionItem {
            label: "__type".to_string(),
            kind: Some(CompletionItemKind::FIELD),
            detail: Some("__Type".to_string()),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: markdown_utils::describe_field_markdown(
                    parent_type.name(),
                    "__type",
                    "__Type",
                    Some("Look up a type definition by its name."),
                ),
            })),
            ..Default::default()
        });
    }
    items
}

/// Get completions for field aliases (after ':' in selections)
pub fn get_alias_completions(
    _doc: &DocumentState,
    parent_type: Option<&str>,
    schema: &Schema,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    if let Some(type_name) = parent_type
        && let Some(ty) = schema.types.get(type_name)
        && let schema::ExtendedType::Object(obj) = ty
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
