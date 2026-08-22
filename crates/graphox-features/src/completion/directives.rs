use apollo_compiler::{Schema, ast};
use graphox_core::document::DocumentState;
use ls_types::{CompletionItem, CompletionItemKind, Documentation, MarkupContent, MarkupKind};
use tree_sitter::Node;

use crate::shared::markdown_utils::describe_directive_markdown;

pub fn get_directive_completions(
    _doc: &DocumentState,
    schema: &Schema,
    location: ast::DirectiveLocation,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for (name, def) in &schema.directive_definitions {
        if def.locations.contains(&location) {
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(CompletionItemKind::FUNCTION),
                documentation: Some(Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: describe_directive_markdown(
                        name,
                        def.description.as_deref(),
                        &def.arguments,
                    ),
                })),
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

pub fn get_directive_argument_completions(
    _doc: &DocumentState,
    directive_name: &str,
    schema: &Schema,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let name = directive_name.strip_prefix('@').unwrap_or(directive_name);

    // Look up directive definition in schema
    if let Some(directive) = schema.directive_definitions.get(name) {
        for arg in &directive.arguments {
            items.push(CompletionItem {
                label: arg.name.to_string(),
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some(format!("{}: {}", arg.name, arg.ty)),
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

    items
}

pub fn try_directive_completions(
    doc: &DocumentState,
    current: Node,
    offset: usize,
    schema: &Schema,
) -> Option<Vec<CompletionItem>> {
    let context_node = find_directive_context_node(doc, current, offset)?;
    let directive_location = find_directive_location(doc, context_node, offset)?;
    Some(get_directive_completions(doc, schema, directive_location))
}

pub fn find_directive_context_node<'a>(
    doc: &DocumentState,
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
    } else if current.kind() == "ERROR" && doc.get_node_text(current, offset) == "@" {
        if let Some(prev) = current.prev_sibling() {
            Some(prev)
        } else {
            Some(current)
        }
    } else {
        Some(current)
    };

    context_node = context_node.and_then(|node| {
        doc.skip_through_kinds(node, &["name", "fragment_name", "ERROR", "MISSING"])
    });

    context_node
}

pub fn find_directive_location<'a>(
    doc: &DocumentState,
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
            "operation_definition" => Some(get_operation_directive_location(doc, p, offset)),
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

pub fn get_operation_directive_location(
    doc: &DocumentState,
    node: Node,
    offset: usize,
) -> ast::DirectiveLocation {
    let op_type_string = doc.get_operation_type(node, offset);
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
