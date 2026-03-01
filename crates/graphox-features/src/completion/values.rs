use crate::completion::{cursor, utils};
use crate::shared::markdown_utils;
use apollo_compiler::{Schema, ast, schema};
use graphox_core::document::DocumentState;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionTextEdit, Documentation, MarkupContent,
    MarkupKind, Range, TextEdit,
};

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
                for (name, val_def) in &e.values {
                    items.push(CompletionItem {
                        label: name.to_string(),
                        kind: Some(CompletionItemKind::ENUM_MEMBER),
                        detail: Some(format!("Enum value of {}", inner_name)),
                        documentation: Some(Documentation::MarkupContent(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: markdown_utils::describe_enum_value_completion_markdown(
                                inner_name.as_str(),
                                val_def.description.as_deref(),
                            ),
                        })),
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
                        value: markdown_utils::describe_type_markdown(name, "interface", Some(d)),
                    })
                }),
                ..Default::default()
            });
        }
    }

    items
}

pub fn get_all_type_completions(schema: &Schema, prefix: Option<&str>) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for (name, def) in &schema.types {
        if name.starts_with("__") {
            continue;
        }
        // Filter by prefix if provided
        if let Some(p) = prefix
            && !name.to_lowercase().starts_with(&p.to_lowercase())
        {
            continue;
        }
        let (kind, type_kind) = match def {
            schema::ExtendedType::Object(_) => (Some(CompletionItemKind::CLASS), "type"),
            schema::ExtendedType::Interface(_) => {
                (Some(CompletionItemKind::INTERFACE), "interface")
            }
            schema::ExtendedType::Enum(_) => (Some(CompletionItemKind::ENUM), "enum"),
            schema::ExtendedType::Union(_) => (Some(CompletionItemKind::INTERFACE), "union"),
            schema::ExtendedType::Scalar(_) => (Some(CompletionItemKind::STRUCT), "scalar"),
            schema::ExtendedType::InputObject(_) => (Some(CompletionItemKind::STRUCT), "input"),
        };
        items.push(CompletionItem {
            label: name.to_string(),
            kind,
            documentation: def.description().map(|d| {
                Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: markdown_utils::describe_type_markdown(name, type_kind, Some(d)),
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
            let (kind, type_kind) = match def {
                schema::ExtendedType::Object(_) => (Some(CompletionItemKind::CLASS), "type"),
                schema::ExtendedType::Interface(_) => {
                    (Some(CompletionItemKind::INTERFACE), "interface")
                }
                schema::ExtendedType::Enum(_) => (Some(CompletionItemKind::ENUM), "enum"),
                schema::ExtendedType::Union(_) => (Some(CompletionItemKind::INTERFACE), "union"),
                schema::ExtendedType::Scalar(_) => (Some(CompletionItemKind::STRUCT), "scalar"),
                schema::ExtendedType::InputObject(_) => (Some(CompletionItemKind::STRUCT), "input"),
            };

            let mut item = CompletionItem {
                label: name.to_string(),
                kind,
                documentation: def.description().map(|d| {
                    Documentation::MarkupContent(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: markdown_utils::describe_type_markdown(name, type_kind, Some(d)),
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
