use crate::completion::{cursor, utils};
use apollo_compiler::{Schema, ast, schema};
use graphox_core::document::DocumentState;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionTextEdit, Documentation, MarkupContent,
    MarkupKind, Range, TextEdit,
};
use tree_sitter::Node;

pub fn get_variable_default_completions(
    _doc: &DocumentState,
    expected_type: &ast::Type,
    schema: &Schema,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    let inner_name = expected_type.inner_named_type();
    let is_non_null = expected_type.is_non_null();
    let is_list = matches!(
        expected_type,
        ast::Type::List(_) | ast::Type::NonNullList(_)
    );

    if let Some(ty) = schema.types.get(inner_name.as_str()) {
        match ty {
            schema::ExtendedType::Enum(e) => {
                for name in e.values.keys() {
                    items.push(CompletionItem {
                        label: name.to_string(),
                        kind: Some(CompletionItemKind::ENUM_MEMBER),
                        detail: Some(format!("Enum value of {}", inner_name)),
                        ..Default::default()
                    });
                }
            }
            schema::ExtendedType::Scalar(s) => {
                if s.name == "Boolean" {
                    items.push(CompletionItem {
                        label: "true".to_string(),
                        kind: Some(CompletionItemKind::KEYWORD),
                        detail: Some("Boolean true".to_string()),
                        ..Default::default()
                    });
                    items.push(CompletionItem {
                        label: "false".to_string(),
                        kind: Some(CompletionItemKind::KEYWORD),
                        detail: Some("Boolean false".to_string()),
                        ..Default::default()
                    });
                } else if s.name == "String" {
                    items.push(CompletionItem {
                        label: "\"\"".to_string(),
                        kind: Some(CompletionItemKind::VALUE),
                        detail: Some("Empty string".to_string()),
                        ..Default::default()
                    });
                }
            }
            _ => {}
        }
    }

    if !is_non_null {
        items.push(CompletionItem {
            label: "null".to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Null value".to_string()),
            ..Default::default()
        });
    }

    if is_list {
        items.push(CompletionItem {
            label: "[]".to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Empty list".to_string()),
            ..Default::default()
        });
    }

    items
}

pub fn get_union_member_completions(schema: &Schema) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    for (name, def) in &schema.types {
        if name.starts_with("__") {
            continue;
        }
        if let schema::ExtendedType::Object(_) = def {
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::CLASS),
                detail: Some("Union member type".to_string()),
                ..Default::default()
            });
        }
    }

    items
}

pub fn get_implements_interface_completions(schema: &Schema) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    for (name, def) in &schema.types {
        if name.starts_with("__") {
            continue;
        }
        if let schema::ExtendedType::Interface(_) = def {
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::INTERFACE),
                detail: Some("Interface to implement".to_string()),
                documentation: def.description().map(|d| {
                    Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: d.to_string(),
                    })
                }),
                ..Default::default()
            });
        }
    }

    items
}

pub fn find_expected_type_for_node(
    doc: &DocumentState,
    node: Node,
    offset: usize,
    cursor_offset: Option<usize>,
    schema: &Schema,
) -> Option<schema::ExtendedType> {
    let ast_type = find_expected_ast_type_for_node(doc, node, offset, cursor_offset, schema)?;
    schema
        .types
        .get(ast_type.inner_named_type().as_str())
        .cloned()
}

pub fn find_expected_ast_type_for_node(
    doc: &DocumentState,
    node: Node,
    offset: usize,
    cursor_offset: Option<usize>,
    schema: &Schema,
) -> Option<ast::Type> {
    let mut curr = Some(node);
    while let Some(current_node) = curr {
        match current_node.kind() {
            "variable_definition" | "input_value_definition" => {
                let mut vd_cursor = current_node.walk();
                let mut var_type_text = None;
                for vd_child in current_node.children(&mut vd_cursor) {
                    if vd_child.kind() == "type" {
                        var_type_text = Some(doc.get_node_text(vd_child, offset));
                        break;
                    }
                }
                if let Some(text) = var_type_text {
                    return Some(utils::parse_type_string(&text));
                }
            }
            "variable_definitions" | "arguments_definition" => {
                if let Some(co) = cursor_offset {
                    let mut cursor = current_node.walk();
                    let mut last_def = None;
                    let target_kind = if current_node.kind() == "variable_definitions" {
                        "variable_definition"
                    } else {
                        "input_value_definition"
                    };

                    for child in current_node.children(&mut cursor) {
                        if child.kind() == target_kind && child.start_byte() + offset < co {
                            last_def = Some(child);
                        }
                    }

                    if let Some(vd) = last_def {
                        let mut vd_cursor = vd.walk();
                        let mut var_type_text = None;
                        for vd_child in vd.children(&mut vd_cursor) {
                            if vd_child.kind() == "type" {
                                var_type_text = Some(doc.get_node_text(vd_child, offset));
                                break;
                            }
                        }
                        if let Some(text) = var_type_text {
                            return Some(utils::parse_type_string(&text));
                        }
                    }
                }
            }
            "argument" | "arguments" => {
                let arg_name = if current_node.kind() == "argument" {
                    doc.find_child_by_kind(current_node, "name")
                        .map(|n| doc.get_node_text(n, offset))
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
                        let text = doc.get_node_text(arg, offset);
                        if let Some(colon_idx) = text.find(':') {
                            let absolute_colon = arg.start_byte() + offset + colon_idx;
                            if co > absolute_colon {
                                doc.find_child_by_kind(arg, "name")
                                    .map(|n| doc.get_node_text(n, offset))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        // If no argument node found, we might be right after a name that isn't yet an argument
                        let text_before = doc
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
                            doc.find_parent_type_for_node(target_node, offset, schema)?;
                        let field_name_node = doc.extract_field_components(target_node).name?;
                        let field_name = doc.get_node_text(field_name_node, offset);

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
                        return Some((*arg_def.ty).clone());
                    } else if target_node.kind() == "directive" {
                        let name_node = doc.find_child_by_kind(target_node, "name")?;
                        let dir_name = doc.get_node_text(name_node, offset);
                        let dir_def = schema.directive_definitions.get(dir_name.as_str())?;
                        let arg_def = dir_def
                            .arguments
                            .iter()
                            .find(|a| a.name.as_str() == arg_name)?;
                        return Some((*arg_def.ty).clone());
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
                        let text = doc.get_node_text(f, offset);
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

                    let field_name_node = doc.find_child_by_kind(f, "name")?;
                    let field_name = doc.get_node_text(field_name_node, offset);

                    let object_value_node = f.parent()?;
                    let parent_input_type = find_expected_type_for_node(
                        doc,
                        object_value_node,
                        offset,
                        cursor_offset,
                        schema,
                    )?;

                    if let schema::ExtendedType::InputObject(input_obj) = parent_input_type {
                        let field_def = input_obj.fields.get(field_name.as_str())?;
                        return Some((*field_def.ty).clone());
                    }
                }
            }
            "list_value" => {
                // Recurse to find the type of the list itself
                let list_type = find_expected_ast_type_for_node(
                    doc,
                    current_node,
                    offset,
                    cursor_offset,
                    schema,
                )?;
                return match list_type {
                    ast::Type::List(inner) => Some(*inner),
                    ast::Type::NonNullList(inner) => Some(*inner),
                    _ => Some(list_type),
                };
            }
            _ => {}
        }
        curr = current_node.parent();
    }
    None
}

pub fn get_all_type_completions(schema: &Schema) -> Vec<CompletionItem> {
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

pub fn get_applicable_type_completions(
    doc: &DocumentState,
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

            let (_prefix_len, start_offset) = cursor::get_prefix_at_cursor(doc, cursor_offset);
            let start_pos = doc.byte_to_position(start_offset);
            let end_pos = doc.byte_to_position(cursor_offset);

            if add_braces {
                if let Some((snippet, format, text_edit)) =
                    utils::create_braced_snippet(doc, name, cursor_offset)
                {
                    item.insert_text = Some(snippet);
                    item.insert_text_format = Some(format);
                    item.text_edit = Some(CompletionTextEdit::Edit(text_edit));
                }
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
