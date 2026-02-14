use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use apollo_compiler::Node;
use apollo_compiler::executable::{self, Selection, SelectionSet};
use apollo_compiler::schema::ExtendedType;

use crate::apply_naming_convention;
use crate::context::CodegenContext;
use crate::helpers::{
    format_union_branches, get_abstract_members, get_typename_value_for_type, gql_type_to_ts,
    wrap_in_list_and_nullability,
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
    used_fragments: &mut HashMap<String, String>,
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
        generate_object_or_intersection(&categorized, parent_type, ctx, indent, used_fragments)
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
    used_fragments: &mut HashMap<String, String>,
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
                used_fragments.insert(spread.fragment_name.to_string(), String::new());
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

fn fields_have_explicit_typename(fields: &[&Node<executable::Field>]) -> bool {
    fields
        .iter()
        .any(|f| f.name.as_str() == "__typename" && f.alias.is_none())
}

/// Generate TypeScript type for object or intersection types (no inline fragments)
fn generate_object_or_intersection(
    categorized: &CategorizedSelections,
    parent_type: &ExtendedType,
    ctx: &CodegenContext,
    indent: usize,
    used_fragments: &mut HashMap<String, String>,
) -> SelectionSetType {
    let local_fields_list = generate_field_list(
        &categorized.fields,
        parent_type,
        ctx,
        indent,
        categorized.has_explicit_typename,
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
    used_fragments: &mut HashMap<String, String>,
) -> Vec<String> {
    let mut local_fields_list = Vec::with_capacity(fields.len() + 1);
    let mut seen_fields: HashSet<String> = HashSet::with_capacity(fields.len() + 1);

    if !has_explicit_typename {
        let typename_value = get_typename_value_for_type(parent_type, ctx.schema);
        local_fields_list.push(format!("__typename: {}", typename_value));
        seen_fields.insert("__typename".to_string());
    }

    for field in fields {
        let name = field.alias.as_ref().unwrap_or(&field.name);

        if !seen_fields.insert(name.to_string()) {
            continue;
        }

        if field.name.as_str() == "__typename" {
            let typename_value = get_typename_value_for_type(parent_type, ctx.schema);
            local_fields_list.push(format!("{}: {}", name, typename_value));
            continue;
        }

        let field_def = match parent_type {
            ExtendedType::Object(obj) => obj.fields.get(field.name.as_str()),
            ExtendedType::Interface(iface) => iface.fields.get(field.name.as_str()),
            _ => None,
        };

        if let Some(fd) = field_def {
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

            local_fields_list.push(format!("{}: {}", name, wrapped_type));
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
        result.push_str(&inner_pad);
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
    has_explicit_typename: bool,
    inline_fragments: &[&Node<executable::InlineFragment>],
    fragment_spreads: &[&Node<executable::FragmentSpread>],
    parent_type: &ExtendedType,
    ctx: &CodegenContext,
    indent: usize,
    used_fragments: &mut HashMap<String, String>,
) -> SelectionSetType {
    let pad = "  ".repeat(indent);
    let mut branches = Vec::with_capacity(inline_fragments.len() + 1);

    // Track which concrete types are covered by inline fragments
    let mut covered_types = HashSet::default();
    for inline in inline_fragments {
        if let Some(type_cond) = &inline.type_condition {
            covered_types.insert(type_cond.as_str());
        }
    }

    // Get all possible concrete types for this abstract type
    let all_members = get_abstract_members(parent_type, ctx.schema);

    // Map each member to the fragment spreads that apply to it
    let mut member_to_spreads: HashMap<&str, Vec<&Node<executable::FragmentSpread>>> =
        HashMap::default();
    for spread in fragment_spreads {
        if let Some(frag_def) = ctx.all_fragments.get(spread.fragment_name.as_str()) {
            let frag_type_name = frag_def.type_condition().as_str();
            if let Some(frag_type) = ctx.schema.types.get(frag_type_name) {
                let frag_members = get_abstract_members(frag_type, ctx.schema);
                for member in frag_members {
                    member_to_spreads.entry(member).or_default().push(spread);
                }
            }
        }
    }

    // Add inline fragment branches
    for inline in inline_fragments {
        let type_name = inline
            .type_condition
            .as_ref()
            .map(|n| n.as_str())
            .unwrap_or_else(|| parent_type.name());

        let empty_vec = Vec::new();
        let applicable_spreads = member_to_spreads.get(type_name).unwrap_or(&empty_vec);

        let branch = generate_inline_fragment_branch(
            fields,
            has_explicit_typename,
            inline,
            applicable_spreads,
            parent_type,
            ctx,
            indent,
            used_fragments,
        );
        branches.push(branch);
    }

    for member in all_members {
        if !covered_types.contains(member) {
            let member_type = ctx.schema.types.get(member).unwrap();
            let member_has_typename = fields_have_explicit_typename(fields);
            let fields_list = generate_field_list(
                fields,
                member_type,
                ctx,
                indent + 1,
                member_has_typename,
                used_fragments,
            );

            let mut type_str = format_multiline_object(&fields_list, indent + 1);

            // Add fragment spreads that apply to this member
            if let Some(spreads) = member_to_spreads.get(member) {
                let spread_str = format_intersection(&[], spreads, ctx);
                type_str = format!("({} & {})", type_str, spread_str);
            }
            branches.push(type_str);
        }
    }

    let type_str = format_union_branches(&branches, &pad);
    SelectionSetType {
        type_str,
        needs_type_declaration: true,
    }
}

/// Generate a single inline fragment branch
#[allow(clippy::too_many_arguments)]
fn generate_inline_fragment_branch(
    common_fields: &[&Node<executable::Field>],
    has_explicit_typename: bool,
    inline: &Node<executable::InlineFragment>,
    parent_fragment_spreads: &[&Node<executable::FragmentSpread>],
    parent_type: &ExtendedType,
    ctx: &CodegenContext,
    indent: usize,
    used_fragments: &mut HashMap<String, String>,
) -> String {
    let type_name = inline
        .type_condition
        .as_ref()
        .map(|n| n.as_str())
        .unwrap_or_else(|| parent_type.name());
    let target_type = ctx.schema.types.get(type_name).unwrap_or(parent_type);

    // Merge common fields with inline fragment's own fields
    let mut all_fields = common_fields.to_vec();
    let mut inline_has_explicit_typename = false;
    for selection in &inline.selection_set.selections {
        if let Selection::Field(field) = selection {
            if field.name.as_str() == "__typename" && field.alias.is_none() {
                inline_has_explicit_typename = true;
            }
            all_fields.push(field);
        }
    }

    let inline_typename = has_explicit_typename || inline_has_explicit_typename;

    let merged_fields_list = generate_field_list(
        &all_fields,
        target_type,
        ctx,
        indent + 1,
        inline_typename,
        used_fragments,
    );

    let mut type_str = format_multiline_object(&merged_fields_list, indent + 1);

    // Collect all fragment spreads: those from parent selection set and those inside this inline fragment
    let mut all_fragment_spreads = parent_fragment_spreads.to_vec();
    for selection in &inline.selection_set.selections {
        if let Selection::FragmentSpread(spread) = selection {
            all_fragment_spreads.push(spread);
            used_fragments.insert(spread.fragment_name.to_string(), String::new());
        }
    }

    if !all_fragment_spreads.is_empty() {
        let spread_str = format_intersection(&[], &all_fragment_spreads, ctx);
        type_str = format!("({} & {})", type_str, spread_str);
    }

    type_str
}
