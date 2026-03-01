use apollo_compiler::{Schema, schema};
use graphox_core::document::DocumentState;
use lsp_types::{CompletionItem, CompletionItemKind, Documentation, MarkupContent, MarkupKind};
use tree_sitter::Node;

use crate::completion::cursor;
use crate::completion::fields;
use crate::completion::types::{FragmentCompletionInfo, FragmentRequirementsResolver};
use crate::completion::values;
use crate::shared::markdown_utils::describe_fragment_completion_markdown;

#[allow(clippy::too_many_arguments)]
pub fn complete_fragment(
    doc: &DocumentState,
    node: Node,
    offset: usize,
    cursor_offset: usize,
    schema: &Schema,
    subgraphs: Option<&[graphox_core::schema::SubgraphInfo]>,
    fragments: &[FragmentCompletionInfo],
    resolve_requirements: FragmentRequirementsResolver,
) -> Option<Vec<CompletionItem>> {
    if let Some(type_cond) = doc.find_child_by_kind(node, "type_condition")
        && doc.is_cursor_in_node_range(type_cond, offset, cursor_offset)
    {
        return Some(values::get_all_type_completions(
            schema,
            cursor::get_word_prefix_at_cursor(doc, cursor_offset).as_deref(),
        ));
    }

    if let Some(selection_set) = doc.find_child_by_kind(node, "selection_set")
        && doc.is_cursor_in_node_range(selection_set, offset, cursor_offset)
        && let Some(type_name) = doc.get_fragment_type_condition(node, offset)
        && let Some(type_def) = schema.types.get(type_name.as_str())
    {
        return fields::complete_selection_set_recursive(
            doc,
            selection_set,
            offset,
            cursor_offset,
            type_def,
            schema,
            subgraphs,
            fragments,
            resolve_requirements,
        );
    }
    None
}

#[allow(clippy::too_many_arguments)]
pub fn complete_inline_fragment(
    doc: &DocumentState,
    node: Node,
    offset: usize,
    cursor_offset: usize,
    schema: &Schema,
    subgraphs: Option<&[graphox_core::schema::SubgraphInfo]>,
    fragments: &[FragmentCompletionInfo],
    resolve_requirements: FragmentRequirementsResolver,
) -> Option<Vec<CompletionItem>> {
    let type_name = doc.get_fragment_type_condition(node, offset);
    let parent_type = if let Some(tn) = type_name {
        schema.types.get(tn.as_str()).cloned()
    } else {
        let mut current = node.parent()?;
        while current.kind() != "selection_set" {
            current = current.parent()?;
        }
        doc.find_parent_type_for_node(node, offset, schema)
    };

    if let Some(type_def) = parent_type
        && let Some(selection_set) = doc.find_child_by_kind(node, "selection_set")
    {
        return fields::complete_selection_set_recursive(
            doc,
            selection_set,
            offset,
            cursor_offset,
            &type_def,
            schema,
            subgraphs,
            fragments,
            resolve_requirements,
        );
    }
    None
}

pub fn find_enclosing_fragment_name(
    doc: &DocumentState,
    node: Node,
    offset: usize,
) -> Option<String> {
    let mut current = node;
    loop {
        if current.kind() == "fragment_definition" {
            return doc
                .find_child_by_kind(current, "fragment_name")
                .map(|name_node| doc.get_node_text(name_node, offset));
        }
        let parent = current.parent()?;
        current = parent;
    }
}

pub fn get_fragment_name_completions(
    _doc: &DocumentState,
    fragments: &[FragmentCompletionInfo],
    expected_type: Option<&schema::ExtendedType>,
    schema: &Schema,
    resolve_requirements: FragmentRequirementsResolver,
    exclude_fragment_name: Option<&str>,
) -> Vec<CompletionItem> {
    fragments
        .iter()
        .filter(|f| {
            if let Some(exclude_name) = exclude_fragment_name
                && f.name.as_ref() == exclude_name
            {
                return false;
            }
            if f.is_type_only {
                return false;
            }
            if let Some(parent) = expected_type {
                let parent_name = parent.name();
                if f.type_condition.as_ref() == parent_name.as_str() {
                    return true;
                }

                match parent {
                    schema::ExtendedType::Object(obj) => {
                        if obj
                            .implements_interfaces
                            .iter()
                            .any(|i| i.as_str() == f.type_condition.as_ref())
                        {
                            return true;
                        }
                    }
                    schema::ExtendedType::Interface(iface) => {
                        if iface
                            .implements_interfaces
                            .iter()
                            .any(|i| i.as_str() == f.type_condition.as_ref())
                        {
                            return true;
                        }
                    }
                    schema::ExtendedType::Union(union) => {
                        if union
                            .members
                            .iter()
                            .any(|m| m.as_str() == f.type_condition.as_ref())
                        {
                            return true;
                        }
                    }
                    _ => {}
                }

                if let Some(frag_type) = schema.types.get(f.type_condition.as_ref())
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
            let requirements_map = resolve_requirements(&f.name);
            let mut documentation = describe_fragment_completion_markdown(
                f.description.as_deref(),
                requirements_map.iter(),
                f.import_path.as_deref(),
            );

            let mut detail = None;
            if let Some(slo) = f.worst_slo {
                detail = Some(format!("Worst SLO: {}", slo.as_str()));
                if documentation.is_empty() {
                    documentation = format!("**Worst SLO:** {}", slo.as_str());
                } else {
                    documentation.push_str("\n\n---\n\n**Worst SLO:** ");
                    documentation.push_str(slo.as_str());
                }
            }

            CompletionItem {
                label: f.name.to_string(),
                kind: Some(CompletionItemKind::SNIPPET),
                detail,
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
