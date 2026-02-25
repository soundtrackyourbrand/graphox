use ahash::AHashSet as HashSet;
use apollo_compiler::Node;
use apollo_compiler::executable::{self, Selection, SelectionSet};
use apollo_compiler::schema::ExtendedType;
use log::warn;
use std::sync::Arc;

use crate::apply_naming_convention;
use crate::context::CodegenContext;
use crate::helpers::{
    format_jsdoc, format_union_branches, gql_type_to_ts, wrap_in_list_and_nullability,
};

#[derive(Debug, Clone)]
pub struct SelectionSetType {
    pub type_str: String,
    pub needs_type_declaration: bool,
}

pub fn generate_selection_set(
    selection_set: &SelectionSet,
    parent_type: &ExtendedType,
    ctx: &CodegenContext,
    indent: usize,
    used_fragments: &mut HashSet<Arc<str>>,
) -> SelectionSetType {
    let categorized = categorize_selections(selection_set, used_fragments);

    let use_union_gen = !categorized.inline_fragments.is_empty()
        || ((matches!(
            parent_type,
            ExtendedType::Union(_) | ExtendedType::Interface(_)
        )) && !categorized.fragment_spreads.is_empty());

    if use_union_gen {
        generate_union_type(
            &categorized.fields,
            categorized.has_explicit_typename,
            &categorized.inline_fragments,
            &categorized.fragment_spreads,
            parent_type,
            ctx,
            indent,
            used_fragments,
        )
    } else {
        generate_object_or_intersection(
            &categorized,
            parent_type,
            ctx,
            indent,
            parent_type,
            used_fragments,
        )
    }
}

/// Categorized results from a selection set
struct CategorizedSelections<'a> {
    fields: Vec<&'a Node<executable::Field>>,
    inline_fragments: Vec<&'a Node<executable::InlineFragment>>,
    fragment_spreads: Vec<&'a Node<executable::FragmentSpread>>,
    has_explicit_typename: bool,
}

/// Categorize selections into fields, inline fragments, and fragment spreads
fn categorize_selections<'a>(
    selection_set: &'a SelectionSet,
    used_fragments: &mut HashSet<Arc<str>>,
) -> CategorizedSelections<'a> {
    let mut fields = Vec::new();
    let mut inline_fragments = Vec::new();
    let mut fragment_spreads = Vec::new();
    let mut has_explicit_typename = false;

    for selection in &selection_set.selections {
        match selection {
            Selection::Field(field) => {
                if field.name.as_str() == "__typename" && field.alias.is_none() {
                    has_explicit_typename = true;
                }
                fields.push(field);
            }
            Selection::InlineFragment(inline) => {
                inline_fragments.push(inline);
            }
            Selection::FragmentSpread(spread) => {
                fragment_spreads.push(spread);
                used_fragments.insert(spread.fragment_name.as_str().into());
            }
        }
    }

    CategorizedSelections {
        fields,
        inline_fragments,
        fragment_spreads,
        has_explicit_typename,
    }
}

/// Generate TypeScript type for object or intersection types (no inline fragments)
fn generate_object_or_intersection(
    categorized: &CategorizedSelections,
    parent_type: &ExtendedType,
    ctx: &CodegenContext,
    indent: usize,
    expected_type: &ExtendedType,
    used_fragments: &mut HashSet<Arc<str>>,
) -> SelectionSetType {
    let local_fields_list = generate_field_list(
        &categorized.fields,
        parent_type,
        ctx,
        indent,
        categorized.has_explicit_typename,
        expected_type,
        used_fragments,
    );

    if categorized.fragment_spreads.is_empty() {
        let type_str = format_multiline_object(&local_fields_list, indent);
        SelectionSetType {
            type_str,
            needs_type_declaration: false,
        }
    } else {
        let type_str = format_intersection(&local_fields_list, &categorized.fragment_spreads, ctx);
        SelectionSetType {
            type_str,
            needs_type_declaration: true,
        }
    }
}

/// Generate list of TypeScript field definitions
fn generate_field_list(
    fields: &[&Node<executable::Field>],
    parent_type: &ExtendedType,
    ctx: &CodegenContext,
    indent: usize,
    has_explicit_typename: bool,
    expected_type: &ExtendedType,
    used_fragments: &mut HashSet<Arc<str>>,
) -> Vec<String> {
    let mut local_fields_list = Vec::with_capacity(fields.len() + 1);
    let mut seen_fields: HashSet<&str> = HashSet::with_capacity(fields.len() + 1);

    if !has_explicit_typename {
        let typename_value =
            ctx.get_typename_value_for_type_with_context(parent_type, expected_type);
        local_fields_list.push(format!("__typename: {}", typename_value));
        seen_fields.insert("__typename");
    }

    for field in fields {
        let name = field.alias.as_ref().unwrap_or(&field.name);

        if !seen_fields.insert(name.as_str()) {
            continue;
        }

        if field.name.as_str() == "__typename" {
            let typename_value =
                ctx.get_typename_value_for_type_with_context(parent_type, expected_type);
            local_fields_list.push(format!("{}: {}", name, typename_value));
            continue;
        }

        let field_def = match parent_type {
            ExtendedType::Object(obj) => obj.fields.get(field.name.as_str()),
            ExtendedType::Interface(iface) => iface.fields.get(field.name.as_str()),
            _ => None,
        };

        if let Some(fd) = field_def {
            let local_deprecation = field.directives.get("deprecated").map(|d| {
                d.argument_by_name("reason", ctx.schema)
                    .ok()
                    .and_then(|v| v.as_str())
                    .unwrap_or("No reason provided")
            });

            let schema_deprecation = fd.directives.get("deprecated").map(|d| {
                d.argument_by_name("reason", ctx.schema)
                    .ok()
                    .and_then(|v| v.as_str())
                    .unwrap_or("No reason provided")
            });

            let deprecation = local_deprecation.or(schema_deprecation);
            let jsdoc = format_jsdoc(fd.description.as_deref(), deprecation, indent + 1);

            let ts_type = if field.selection_set.selections.is_empty() {
                gql_type_to_ts(&fd.ty, ctx.schema, ctx.scalars, ctx)
            } else {
                let inner_type_name = fd.ty.inner_named_type();
                let inner_type = ctx
                    .schema
                    .types
                    .get(inner_type_name.as_str())
                    .expect("Field type must exist");
                let result = generate_selection_set(
                    &field.selection_set,
                    inner_type,
                    ctx,
                    indent + 1,
                    used_fragments,
                );
                result.type_str
            };

            let wrapped_type = if field.selection_set.selections.is_empty() {
                ts_type
            } else {
                wrap_in_list_and_nullability(&ts_type, &fd.ty)
            };

            let field_line = if jsdoc.is_empty() {
                format!("{}: {}", name, wrapped_type)
            } else {
                let inner_pad = "  ".repeat(indent + 1);
                format!("{}{}{}: {}", jsdoc, inner_pad, name, wrapped_type)
            };
            local_fields_list.push(field_line);
        }
    }

    local_fields_list
}

/// Format fields as a multi-line TypeScript object
fn format_multiline_object(fields: &[String], indent: usize) -> String {
    let pad = "  ".repeat(indent);
    let inner_pad = "  ".repeat(indent + 1);

    let estimated_size = fields.len() * 40 + 20;
    let mut result = String::with_capacity(estimated_size);

    for f in fields {
        result.push('\n');
        if !f.starts_with(&inner_pad) {
            result.push_str(&inner_pad);
        }
        result.push_str(f);
        result.push(';');
    }
    result.push('\n');
    result.push_str(&pad);

    let mut output = String::with_capacity(result.len() + 2);
    output.push('{');
    output.push_str(&result);
    output.push('}');
    output
}

/// Format as TypeScript intersection type (object & fragments) or FragmentType wrapper
fn format_intersection(
    fields: &[String],
    fragment_spreads: &[&Node<executable::FragmentSpread>],
    ctx: &CodegenContext,
) -> String {
    if fragment_spreads.is_empty() {
        return format!("{{ {} }}", fields.join(", "));
    }

    let result = if ctx.fragment_masking().is_enabled() {
        let mut refs: Vec<_> = fragment_spreads
            .iter()
            .map(|s| {
                let name = format!(
                    "{}{}",
                    apply_naming_convention(s.fragment_name.as_str(), &ctx.naming_convention()),
                    ctx.fragment_suffix()
                );
                format!("'{}': {}", name, name)
            })
            .collect();
        refs.sort();

        let refs_obj = format!("{{ ' $fragmentRefs'?: {{ {} }} }}", refs.join(", "));

        if fields.is_empty() {
            refs_obj
        } else {
            let base_obj = format!("{{ {} }}", fields.join(", "));
            format!("({} & {})", base_obj, refs_obj)
        }
    } else {
        let mut plain_spreads: Vec<_> = fragment_spreads
            .iter()
            .map(|s| {
                format!(
                    "{}{}",
                    apply_naming_convention(s.fragment_name.as_str(), &ctx.naming_convention()),
                    ctx.fragment_suffix()
                )
            })
            .collect();
        plain_spreads.sort();
        let spreads_str = if plain_spreads.len() > 1 {
            format!("({})", plain_spreads.join(" & "))
        } else {
            plain_spreads[0].clone()
        };

        if fields.is_empty() {
            spreads_str
        } else {
            let base_obj = format!("{{ {} }}", fields.join(", "));
            format!("({} & {})", base_obj, spreads_str)
        }
    };

    // Wrap intersection types with Identity<> for better TypeScript inference
    if fragment_spreads.len() > 1 || (ctx.fragment_masking().is_enabled() && !fields.is_empty()) {
        format!("Identity<{}>", result)
    } else {
        result
    }
}

/// Generate TypeScript union type for inline fragments
#[allow(clippy::too_many_arguments)]
fn generate_union_type(
    fields: &[&Node<executable::Field>],
    _has_explicit_typename: bool,
    inline_fragments: &[&Node<executable::InlineFragment>],
    fragment_spreads: &[&Node<executable::FragmentSpread>],
    parent_type: &ExtendedType,
    ctx: &CodegenContext,
    indent: usize,
    used_fragments: &mut HashSet<Arc<str>>,
) -> SelectionSetType {
    let pad = "  ".repeat(indent);

    struct SelectionGroup {
        fields_list: Vec<String>,
        spreads_str: String,
        members: Vec<Arc<str>>,
    }
    let mut groups: Vec<SelectionGroup> = Vec::new();

    let all_members = ctx.get_abstract_members(parent_type.name());

    for member_name in all_members {
        let Some(member_type) = ctx.schema.types.get(member_name.as_ref()) else {
            warn!(
                "Union/interface member type '{}' not found in schema (referenced in: {:?})",
                member_name, ctx.codegen_path
            );
            continue;
        };

        // 1. Collect all fields applicable to this specific member
        let mut member_fields = fields.to_vec();
        for inline in inline_fragments {
            let cond = inline
                .type_condition
                .as_ref()
                .map(|n| n.as_str())
                .unwrap_or_else(|| parent_type.name());
            if ctx.is_type_applicable(member_name.as_ref(), cond) {
                for selection in &inline.selection_set.selections {
                    if let Selection::Field(f) = selection {
                        member_fields.push(f);
                    }
                }
            }
        }

        // 2. Collect all spreads applicable to this specific member
        let mut member_spreads = Vec::new();
        // Spreads from the base selection set
        for spread in fragment_spreads {
            if let Some(frag_def) = ctx.all_fragments.get(spread.fragment_name.as_str())
                && ctx.is_type_applicable(member_name.as_ref(), frag_def.type_condition().as_str())
            {
                member_spreads.push(*spread);
            }
        }
        // Spreads from inside inline fragments
        for inline in inline_fragments {
            let cond = inline
                .type_condition
                .as_ref()
                .map(|n| n.as_str())
                .unwrap_or_else(|| parent_type.name());
            if ctx.is_type_applicable(member_name.as_ref(), cond) {
                for selection in &inline.selection_set.selections {
                    if let Selection::FragmentSpread(s) = selection {
                        member_spreads.push(s);
                        used_fragments.insert(s.fragment_name.as_str().into());
                    }
                }
            }
        }

        // 3. Generate field list for this member
        // We pass has_explicit_typename: true to skip automatic __typename generation
        // for individual members, we'll add the combined one for the group below.
        let mut fields_list = generate_field_list(
            &member_fields,
            member_type,
            ctx,
            indent + 1,
            true, // Skip auto __typename
            parent_type,
            used_fragments,
        );

        // Remove any explicit __typename that might have been matched
        fields_list.retain(|f| !f.starts_with("__typename:"));

        let spreads_str = if member_spreads.is_empty() {
            String::new()
        } else {
            format_intersection(&[], &member_spreads, ctx)
        };

        if let Some(existing) = groups
            .iter_mut()
            .find(|g| g.fields_list == fields_list && g.spreads_str == spreads_str)
        {
            existing.members.push(member_name.clone());
        } else {
            groups.push(SelectionGroup {
                fields_list,
                spreads_str,
                members: vec![member_name.clone()],
            });
        }
    }

    let mut branches = Vec::new();
    for group in groups {
        let mut final_fields = group.fields_list;

        // Prepend the combined __typename for this group
        let typename_value = group
            .members
            .iter()
            .map(|m| format!("\"{}\"", m))
            .collect::<Vec<_>>()
            .join(" | ");
        final_fields.insert(0, format!("__typename: {}", typename_value));

        let mut type_str = format_multiline_object(&final_fields, indent + 1);
        if !group.spreads_str.is_empty() {
            type_str = format!("({} & {})", type_str, group.spreads_str);
        }
        branches.push(type_str);
    }

    let type_str = format_union_branches(&branches, &pad);
    SelectionSetType {
        type_str,
        needs_type_declaration: false,
    }
}
