use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use apollo_compiler::Schema;
use apollo_compiler::ast::Type;
use apollo_compiler::executable::{self, Selection};
use apollo_compiler::schema::ExtendedType;
use graphox_core::apollo_ast::get_fragment_fragment_dependencies;
use std::sync::Arc;

use crate::context::CodegenContext;

pub fn gql_type_to_ts(
    ty: &Type,
    schema: &Schema,
    scalars: &HashMap<String, String>,
    ctx: &CodegenContext,
) -> String {
    gql_type_to_ts_internal(ty, schema, false, scalars, ctx)
}

pub fn gql_type_to_ts_with_names(
    ty: &Type,
    schema: &Schema,
    scalars: &HashMap<String, String>,
    ctx: &CodegenContext,
) -> String {
    gql_type_to_ts_internal(ty, schema, true, scalars, ctx)
}

fn gql_type_to_ts_internal(
    ty: &Type,
    schema: &Schema,
    use_names: bool,
    scalars: &HashMap<String, String>,
    ctx: &CodegenContext,
) -> String {
    let inner_name = ty.inner_named_type();

    if ctx.schema_import.is_some() || ctx.type_imports.contains_key(inner_name.as_str()) {
        let is_builtin = matches!(
            inner_name.as_str(),
            "String" | "Int" | "Float" | "Boolean" | "ID"
        );

        if !is_builtin && let Some(t) = schema.types.get(inner_name.as_str()) {
            match t {
                ExtendedType::Enum(_) | ExtendedType::InputObject(_) | ExtendedType::Scalar(_) => {
                    ctx.used_schema_types
                        .borrow_mut()
                        .insert(inner_name.to_string());
                }
                ExtendedType::Object(_) | ExtendedType::Interface(_) | ExtendedType::Union(_) => {
                    if use_names {
                        ctx.used_schema_types
                            .borrow_mut()
                            .insert(inner_name.to_string());
                    }
                }
            }
        }
    }

    let base = match inner_name.as_str() {
        "String" | "ID" => "string".to_string(),
        "Int" | "Float" => "number".to_string(),
        "Boolean" => "boolean".to_string(),
        other => ctx.get_cached_type(other, || {
            if let Some(mapped) = scalars.get(other) {
                mapped.to_string()
            } else if let Some(t) = schema.types.get(other) {
                match t {
                    ExtendedType::Enum(enm) => {
                        if ctx.schema_import.is_some()
                            || ctx.type_imports.contains_key(other)
                            || use_names
                        {
                            other.to_string()
                        } else {
                            let mut values: Vec<_> = enm.values.keys().collect();
                            values.sort();
                            values
                                .iter()
                                .map(|v| format!("\"{}\"", v))
                                .collect::<Vec<_>>()
                                .join(" | ")
                        }
                    }
                    ExtendedType::InputObject(_) | ExtendedType::Scalar(_) => {
                        if ctx.schema_import.is_some()
                            || ctx.type_imports.contains_key(other)
                            || use_names
                        {
                            other.to_string()
                        } else {
                            "any".to_string()
                        }
                    }
                    ExtendedType::Object(_)
                    | ExtendedType::Interface(_)
                    | ExtendedType::Union(_) => {
                        if ctx.schema_import.is_some()
                            || ctx.type_imports.contains_key(other)
                            || use_names
                        {
                            other.to_string()
                        } else {
                            "any".to_string()
                        }
                    }
                }
            } else {
                "any".to_string()
            }
        }),
    };

    generate_ts_type(ty, &base)
}

pub fn generate_ts_type(ty: &Type, base: &str) -> String {
    wrap_type_recursive(ty, base)
}

pub fn wrap_in_list_and_nullability(base: &str, ty: &Type) -> String {
    wrap_type_recursive(ty, base)
}

fn wrap_type_recursive(ty: &Type, base: &str) -> String {
    match ty {
        Type::Named(_) => format!("{} | null", base),
        Type::NonNullNamed(_) => base.to_string(),
        Type::List(inner) => format!("Array<{}> | null", wrap_type_recursive(inner, base)),
        Type::NonNullList(inner) => format!("Array<{}>", wrap_type_recursive(inner, base)),
    }
}

pub fn format_jsdoc(
    description: Option<&str>,
    deprecation_reason: Option<&str>,
    indent_level: usize,
) -> String {
    let has_desc = description.is_some_and(|d| !d.trim().is_empty());
    let is_deprecated = deprecation_reason.is_some();

    if !has_desc && !is_deprecated {
        return String::new();
    }

    let indent = "  ".repeat(indent_level);
    let mut jsdoc = String::new();
    jsdoc.push_str(&format!("{}/**\n", indent));

    if let Some(desc) = description {
        for line in desc.lines() {
            jsdoc.push_str(&format!("{} * {}\n", indent, line.trim()));
        }
    }

    if let Some(reason) = deprecation_reason {
        if has_desc {
            jsdoc.push_str(&format!("{} *\n", indent));
        }
        jsdoc.push_str(&format!("{} * @deprecated {}\n", indent, reason));
    }

    jsdoc.push_str(&format!("{} */\n", indent));
    jsdoc
}

pub fn get_abstract_members<'a>(ty: &'a ExtendedType, schema: &'a Schema) -> Vec<&'a str> {
    match ty {
        ExtendedType::Union(union) => union.members.iter().map(|m| m.as_str()).collect(),
        ExtendedType::Interface(_) => schema
            .types
            .iter()
            .filter_map(|(name, t)| {
                if let ExtendedType::Object(obj) = t
                    && obj
                        .implements_interfaces
                        .iter()
                        .any(|i| i.as_str() == ty.name().as_str())
                {
                    return Some(name.as_str());
                }
                None
            })
            .collect(),
        _ => vec![ty.name().as_str()],
    }
}

pub fn get_interface_implementors(interface_name: &str, schema: &Schema) -> Vec<String> {
    schema
        .types
        .iter()
        .filter_map(|(n, t)| {
            if let ExtendedType::Object(obj) = t {
                if obj
                    .implements_interfaces
                    .iter()
                    .any(|i| i.as_str() == interface_name)
                {
                    Some(format!("\"{}\"", n))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}

pub fn get_typename_value_for_type(parent_type: &ExtendedType, schema: &Schema) -> String {
    match parent_type {
        ExtendedType::Interface(iface) => {
            let implementors = get_interface_implementors(&iface.name, schema);
            if implementors.is_empty() {
                "string".to_string()
            } else {
                implementors.join(" | ")
            }
        }
        _ => format!("\"{}\"", parent_type.name()),
    }
}

pub fn format_union_branches(branches: &[String], pad: &str) -> String {
    let mut result = branches[0].clone();
    for (i, branch) in branches.iter().enumerate().skip(1) {
        if i == 1 {
            result.push('\n');
            result.push_str(pad);
            result.push_str("  | ");
        } else {
            result.push_str(" | ");
        }
        result.push_str(branch);
    }
    result
}

pub fn get_operation_deps_cached(
    operation: &executable::Operation,
    ctx: &CodegenContext,
) -> HashSet<Arc<str>> {
    let mut all_deps = HashSet::default();
    collect_direct_fragment_spreads(&operation.selection_set, &mut all_deps);

    let initial_size = all_deps.len();
    let mut transitive_deps: HashSet<Arc<str>> =
        HashSet::with_capacity_and_hasher(initial_size * 2, Default::default());

    for frag_name in &all_deps {
        if let Some(cached_transitive) = ctx.fragment_dependencies.get(&frag_name[..]) {
            transitive_deps.extend(cached_transitive.iter().cloned());
        } else if let Some(parsed_frag) = ctx.all_fragments.get(frag_name.as_ref()) {
            let frag_deps = get_fragment_fragment_dependencies(parsed_frag, ctx.all_fragments);
            transitive_deps.extend(frag_deps.into_iter());
        }
    }

    all_deps.extend(transitive_deps);
    all_deps
}

pub fn get_fragment_deps_cached(fragment_name: &str, ctx: &CodegenContext) -> HashSet<Arc<str>> {
    if let Some(cached_deps) = ctx.fragment_dependencies.get(fragment_name) {
        cached_deps.iter().cloned().collect()
    } else {
        HashSet::default()
    }
}

pub fn collect_direct_fragment_spreads(
    selection_set: &executable::SelectionSet,
    spreads: &mut HashSet<Arc<str>>,
) {
    for selection in &selection_set.selections {
        match selection {
            Selection::Field(field) => {
                collect_direct_fragment_spreads(&field.selection_set, spreads);
            }
            Selection::InlineFragment(inline) => {
                collect_direct_fragment_spreads(&inline.selection_set, spreads);
            }
            Selection::FragmentSpread(spread) => {
                spreads.insert(spread.fragment_name.as_str().into());
            }
        }
    }
}
