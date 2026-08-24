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
#[derive(Default)]
struct CategorizedSelections<'a> {
    fields: Vec<&'a Node<executable::Field>>,
    inline_fragments: Vec<&'a Node<executable::InlineFragment>>,
    fragment_spreads: Vec<&'a Node<executable::FragmentSpread>>,
    has_explicit_typename: bool,
}

impl<'a> CategorizedSelections<'a> {
    fn add(&mut self, selection: &'a Selection) {
        match selection {
            Selection::Field(field) => {
                if field.name.as_str() == "__typename" && field.alias.is_none() {
                    self.has_explicit_typename = true;
                }
                self.fields.push(field);
            }
            Selection::InlineFragment(inline) => {
                self.inline_fragments.push(inline);
            }
            Selection::FragmentSpread(spread) => {
                self.fragment_spreads.push(spread);
            }
        }
    }
}

/// Categorize selections into fields, inline fragments, and fragment spreads
fn categorize_selections<'a>(
    selection_set: &'a SelectionSet,
    used_fragments: &mut HashSet<Arc<str>>,
) -> CategorizedSelections<'a> {
    let mut categorized = CategorizedSelections::default();

    for selection in &selection_set.selections {
        categorized.add(selection);
        if let Selection::FragmentSpread(spread) = selection {
            used_fragments.insert(spread.fragment_name.as_str().into());
        }
    }

    categorized
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
    let merged_keys = collect_list_collisions(
        &categorized.fields,
        &categorized.fragment_spreads,
        parent_type,
        ctx,
    );

    let local_fields_list = generate_field_list(
        &categorized.fields,
        parent_type,
        ctx,
        indent,
        categorized.has_explicit_typename,
        expected_type,
        used_fragments,
        &merged_keys,
    );

    if categorized.fragment_spreads.is_empty() {
        let type_str = format_multiline_object(&local_fields_list, indent);
        SelectionSetType {
            type_str,
            needs_type_declaration: false,
        }
    } else {
        let type_str = format_intersection(
            &local_fields_list,
            &categorized.fragment_spreads,
            ctx,
            &merged_keys,
        );
        SelectionSetType {
            type_str,
            needs_type_declaration: true,
        }
    }
}

/// A response key this selection set and a spread fragment both select.
///
/// TypeScript intersects the two property types. For an object that is what we
/// want — `{ a } & { b }` has both members — but a list becomes
/// `Array<A> & Array<B>`, and `.map` binds only the first constituent, so the
/// other's fields silently disappear from the callback parameter. Colliding list
/// keys are therefore generated as one merged property and omitted from the
/// fragment types they also came from.
struct MergedKey {
    key: String,
    /// The field to take the name, schema type and JSDoc from.
    field: Node<executable::Field>,
    /// Every contributor's sub-selections, generated as one selection set.
    selections: Vec<Selection>,
    /// Fragments whose type must no longer carry the key.
    omit_from: Vec<Arc<str>>,
    /// Whether this selection set selects the key itself, or only fragments do.
    selected_locally: bool,
}

/// Collect a fragment's top-level response keys, following its own spreads and
/// inline fragments, since all of them land in the same generated type.
fn collect_fragment_fields<'a>(
    selection_set: &'a SelectionSet,
    ctx: &'a CodegenContext,
    visited: &mut HashSet<Arc<str>>,
    out: &mut Vec<&'a Node<executable::Field>>,
) {
    for selection in &selection_set.selections {
        match selection {
            Selection::Field(field) => out.push(field),
            Selection::InlineFragment(inline) => {
                collect_fragment_fields(&inline.selection_set, ctx, visited, out)
            }
            Selection::FragmentSpread(spread) => {
                let name: Arc<str> = spread.fragment_name.as_str().into();
                if !visited.insert(name.clone()) {
                    continue;
                }
                if let Some(frag) = ctx.all_fragments.get(spread.fragment_name.as_str()) {
                    collect_fragment_fields(&frag.selection_set, ctx, visited, out);
                }
            }
        }
    }
}

fn response_key(field: &Node<executable::Field>) -> &str {
    field.alias.as_ref().unwrap_or(&field.name).as_str()
}

fn field_is_list(parent_type: &ExtendedType, field: &Node<executable::Field>) -> bool {
    let field_def = match parent_type {
        ExtendedType::Object(obj) => obj.fields.get(field.name.as_str()),
        ExtendedType::Interface(iface) => iface.fields.get(field.name.as_str()),
        _ => None,
    };
    field_def.is_some_and(|fd| fd.ty.is_list())
}

fn collect_list_collisions(
    fields: &[&Node<executable::Field>],
    fragment_spreads: &[&Node<executable::FragmentSpread>],
    parent_type: &ExtendedType,
    ctx: &CodegenContext,
) -> Vec<MergedKey> {
    if fragment_spreads.is_empty() {
        return Vec::new();
    }

    // key -> (contributing fields, fragments to omit it from, selected here)
    let mut contributors: Vec<MergedKey> = Vec::new();
    let mut index: ahash::AHashMap<String, usize> = ahash::AHashMap::default();

    let record = |key: &str,
                  field: &Node<executable::Field>,
                  from_fragment: Option<&Arc<str>>,
                  contributors: &mut Vec<MergedKey>,
                  index: &mut ahash::AHashMap<String, usize>| {
        let slot = *index.entry(key.to_string()).or_insert_with(|| {
            contributors.push(MergedKey {
                key: key.to_string(),
                field: field.clone(),
                selections: Vec::new(),
                omit_from: Vec::new(),
                selected_locally: from_fragment.is_none(),
            });
            contributors.len() - 1
        });
        let entry = &mut contributors[slot];
        entry
            .selections
            .extend(field.selection_set.selections.iter().cloned());
        match from_fragment {
            Some(name) => {
                if !entry.omit_from.contains(name) {
                    entry.omit_from.push(name.clone());
                }
            }
            None => entry.selected_locally = true,
        }
    };

    for field in fields {
        if field.selection_set.selections.is_empty() {
            continue;
        }
        record(
            response_key(field),
            field,
            None,
            &mut contributors,
            &mut index,
        );
    }

    for spread in fragment_spreads {
        let Some(frag) = ctx.all_fragments.get(spread.fragment_name.as_str()) else {
            continue;
        };
        let spread_name: Arc<str> = spread.fragment_name.as_str().into();
        let mut fields = Vec::new();
        let mut visited = HashSet::default();
        visited.insert(spread_name.clone());
        collect_fragment_fields(&frag.selection_set, ctx, &mut visited, &mut fields);
        for field in fields {
            if field.selection_set.selections.is_empty() {
                continue;
            }
            record(
                response_key(field),
                field,
                Some(&spread_name),
                &mut contributors,
                &mut index,
            );
        }
    }

    contributors.retain(|c| {
        let contributor_count = c.omit_from.len() + if c.selected_locally { 1 } else { 0 };
        contributor_count > 1 && field_is_list(parent_type, &c.field)
    });
    contributors
}

/// Generate list of TypeScript field definitions
#[allow(clippy::too_many_arguments)]
fn generate_field_list(
    fields: &[&Node<executable::Field>],
    parent_type: &ExtendedType,
    ctx: &CodegenContext,
    indent: usize,
    has_explicit_typename: bool,
    expected_type: &ExtendedType,
    used_fragments: &mut HashSet<Arc<str>>,
    merged_keys: &[MergedKey],
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

            let merged = merged_keys.iter().find(|m| m.key == name.as_str());

            let ts_type = if field.selection_set.selections.is_empty() {
                gql_type_to_ts(&fd.ty, ctx.schema, ctx.scalars, ctx)
            } else {
                let inner_type_name = fd.ty.inner_named_type();
                let inner_type = ctx
                    .schema
                    .types
                    .get(inner_type_name.as_str())
                    .expect("Field type must exist");
                // A colliding list key is generated from every contributor at
                // once, so the element type has all their fields.
                let merged_set = merged.map(|m| SelectionSet {
                    ty: field.selection_set.ty.clone(),
                    selections: m.selections.clone(),
                });
                let result = generate_selection_set(
                    merged_set.as_ref().unwrap_or(&field.selection_set),
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

            let optional_marker = if matches!(
                &fd.ty,
                apollo_compiler::schema::Type::Named(_) | apollo_compiler::schema::Type::List(_)
            ) && ctx.nullable_fields_as_optional()
            {
                "?"
            } else {
                ""
            };

            let field_line = if jsdoc.is_empty() {
                format!("{}{}: {}", name, optional_marker, wrapped_type)
            } else {
                let inner_pad = "  ".repeat(indent + 1);
                format!(
                    "{}{}{}{}: {}",
                    jsdoc, inner_pad, name, optional_marker, wrapped_type
                )
            };
            local_fields_list.push(field_line);
        }
    }

    // Keys only the fragments selected have no field here to hang the merged
    // property on, so they are appended.
    for merged in merged_keys {
        if merged.selected_locally || seen_fields.contains(merged.key.as_str()) {
            continue;
        }
        if let Some(line) = render_merged_field(merged, parent_type, ctx, indent, used_fragments) {
            local_fields_list.push(line);
        }
    }

    local_fields_list
}

/// Render one merged property for a key this selection set does not select
/// itself, generated from the contributing fragments' selections.
fn render_merged_field(
    merged: &MergedKey,
    parent_type: &ExtendedType,
    ctx: &CodegenContext,
    indent: usize,
    used_fragments: &mut HashSet<Arc<str>>,
) -> Option<String> {
    let field_def = match parent_type {
        ExtendedType::Object(obj) => obj.fields.get(merged.field.name.as_str()),
        ExtendedType::Interface(iface) => iface.fields.get(merged.field.name.as_str()),
        _ => None,
    }?;

    let inner_type = ctx
        .schema
        .types
        .get(field_def.ty.inner_named_type().as_str())?;
    let selection_set = SelectionSet {
        ty: merged.field.selection_set.ty.clone(),
        selections: merged.selections.clone(),
    };
    let generated =
        generate_selection_set(&selection_set, inner_type, ctx, indent + 1, used_fragments);
    let wrapped = wrap_in_list_and_nullability(&generated.type_str, &field_def.ty);

    let optional_marker = if matches!(
        &field_def.ty,
        apollo_compiler::schema::Type::Named(_) | apollo_compiler::schema::Type::List(_)
    ) && ctx.nullable_fields_as_optional()
    {
        "?"
    } else {
        ""
    };

    Some(format!("{}{}: {}", merged.key, optional_marker, wrapped))
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
    merged_keys: &[MergedKey],
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
                let type_name = format!(
                    "{}{}",
                    apply_naming_convention(s.fragment_name.as_str(), &ctx.naming_convention()),
                    ctx.fragment_suffix()
                );
                // Keys generated as one merged property must not also arrive
                // through the fragment, or the intersection is back.
                let mut omitted: Vec<&str> = merged_keys
                    .iter()
                    .filter(|m| {
                        m.omit_from
                            .iter()
                            .any(|f| f.as_ref() == s.fragment_name.as_str())
                    })
                    .map(|m| m.key.as_str())
                    .collect();
                if omitted.is_empty() {
                    return type_name;
                }
                omitted.sort();
                let keys = omitted
                    .iter()
                    .map(|k| format!("'{}'", k))
                    .collect::<Vec<_>>()
                    .join(" | ");
                format!("Omit<{}, {}>", type_name, keys)
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
        key: SelectionKey,
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
        let member_merged =
            collect_list_collisions(&member_fields, &member_spreads, member_type, ctx);

        let fields_list = generate_field_list(
            &member_fields,
            member_type,
            ctx,
            indent + 1,
            true, // Skip auto __typename
            parent_type,
            used_fragments,
            &member_merged,
        );

        let spreads_str = if member_spreads.is_empty() {
            String::new()
        } else {
            format_intersection(&[], &member_spreads, ctx, &member_merged)
        };

        // 4. Collect all selections applicable to this specific member for structural key generation
        let mut member_selections = Vec::new();
        for selection in fields {
            member_selections.push(Selection::Field((*selection).clone()));
        }
        for selection in &member_spreads {
            member_selections.push(Selection::FragmentSpread((*selection).clone()));
        }
        for inline in inline_fragments {
            let cond = inline
                .type_condition
                .as_ref()
                .map(|n| n.as_str())
                .unwrap_or_else(|| parent_type.name());
            if ctx.is_type_applicable(member_name.as_ref(), cond) {
                member_selections.push(Selection::InlineFragment((*inline).clone()));
            }
        }

        let key = generate_selection_key(&member_selections, member_type, ctx);

        if ctx.merge_union_types() {
            if let Some(existing) = groups.iter_mut().find(|g| g.key == key) {
                existing.members.push(member_name.clone());
            } else {
                groups.push(SelectionGroup {
                    fields_list,
                    spreads_str,
                    members: vec![member_name.clone()],
                    key,
                });
            }
        } else {
            groups.push(SelectionGroup {
                fields_list,
                spreads_str,
                members: vec![member_name.clone()],
                key,
            });
        }
    }

    let mut branches = Vec::new();
    for group in groups {
        let mut final_fields = group.fields_list;

        // Remove any explicit __typename that might have been matched
        final_fields.retain(|f| !f.starts_with("__typename:"));

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
    let needs_type_declaration = branches.len() > 1 || branches.iter().any(|b| b.contains('&'));

    SelectionSetType {
        type_str,
        needs_type_declaration,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct SelectionKey {
    fields: Vec<(String, String)>, // (alias/name, structural_type_str)
    spreads: Vec<String>,          // fragment names
    inline_fragments: Vec<(String, Box<SelectionKey>)>,
}

fn merge_selection_key(
    field_keys: &mut Vec<(String, String)>,
    spread_keys: &mut HashSet<String>,
    inline_fragment_keys: &mut Vec<(String, Box<SelectionKey>)>,
    seen_fields: &mut HashSet<String>,
    sub_key: SelectionKey,
) {
    for (name, ty) in sub_key.fields {
        if seen_fields.insert(name.clone()) {
            field_keys.push((name, ty));
        }
    }
    for spread in sub_key.spreads {
        spread_keys.insert(spread);
    }
    inline_fragment_keys.extend(sub_key.inline_fragments);
}

fn generate_selection_key(
    selections: &[Selection],
    parent_type: &ExtendedType,
    ctx: &CodegenContext,
) -> SelectionKey {
    let mut field_keys = Vec::new();
    let mut spread_keys = HashSet::new();
    let mut inline_fragment_keys = Vec::new();
    let mut seen_fields = HashSet::new();

    for selection in selections {
        match selection {
            Selection::Field(field) => {
                let name = field.alias.as_ref().unwrap_or(&field.name).as_str();
                if !seen_fields.insert(name.to_string()) {
                    continue;
                }
                if name == "__typename" {
                    continue;
                }

                let field_def = match parent_type {
                    ExtendedType::Object(obj) => obj.fields.get(field.name.as_str()),
                    ExtendedType::Interface(iface) => iface.fields.get(field.name.as_str()),
                    _ => None,
                };

                if let Some(fd) = field_def {
                    let mut type_key = String::new();
                    if field.selection_set.selections.is_empty() {
                        type_key = gql_type_to_ts(&fd.ty, ctx.schema, ctx.scalars, ctx);
                    } else {
                        let inner_type_name = fd.ty.inner_named_type();
                        if let Some(inner_type) = ctx.schema.types.get(inner_type_name.as_str()) {
                            let sub_key = generate_selection_key(
                                &field.selection_set.selections,
                                inner_type,
                                ctx,
                            );
                            type_key = format!("{:?}", sub_key);
                            type_key = wrap_in_list_and_nullability(&type_key, &fd.ty);
                        }
                    }
                    if ctx.nullable_fields_as_optional()
                        && matches!(
                            &fd.ty,
                            apollo_compiler::schema::Type::Named(_)
                                | apollo_compiler::schema::Type::List(_)
                        )
                    {
                        type_key.push('?');
                    }
                    field_keys.push((name.to_string(), type_key));
                }
            }
            Selection::FragmentSpread(spread) => {
                spread_keys.insert(spread.fragment_name.as_str().to_string());
            }
            Selection::InlineFragment(inline) => {
                let effective_parent = inline
                    .type_condition
                    .as_ref()
                    .and_then(|condition| ctx.schema.types.get(condition.as_str()))
                    .unwrap_or(parent_type);
                let sub_key =
                    generate_selection_key(&inline.selection_set.selections, effective_parent, ctx);

                let should_preserve_condition = matches!(
                    parent_type,
                    ExtendedType::Union(_) | ExtendedType::Interface(_)
                ) && inline
                    .type_condition
                    .as_ref()
                    .is_some_and(|condition| condition.as_str() != parent_type.name().as_str());

                if let Some(condition) = inline.type_condition.as_ref()
                    && should_preserve_condition
                {
                    inline_fragment_keys.push((condition.as_str().to_string(), Box::new(sub_key)));
                } else {
                    merge_selection_key(
                        &mut field_keys,
                        &mut spread_keys,
                        &mut inline_fragment_keys,
                        &mut seen_fields,
                        sub_key,
                    );
                }
            }
        }
    }
    field_keys.sort_by(|a, b| a.0.cmp(&b.0));

    let mut sorted_spreads: Vec<_> = spread_keys.into_iter().collect();
    sorted_spreads.sort();

    inline_fragment_keys.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| format!("{:?}", a.1).cmp(&format!("{:?}", b.1)))
    });

    SelectionKey {
        fields: field_keys,
        spreads: sorted_spreads,
        inline_fragments: inline_fragment_keys,
    }
}
