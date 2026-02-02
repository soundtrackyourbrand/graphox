use crate::document::DocumentState;
use apollo_compiler::{executable, Schema};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub struct CodegenContext<'a> {
    pub schema: &'a Schema,
    pub fragment_to_path: &'a HashMap<String, String>,
    pub current_file_path: &'a Path,
}

pub fn generate_typescript(doc: &DocumentState, ctx: &CodegenContext) -> Result<String, String> {
    let mut body = String::new();
    let mut used_fragments = HashSet::new();

    let mut combined_source = String::new();
    for block in doc.get_graphql_trees() {
        let start = block.offset;
        let end = block.offset + block.tree.root_node().end_byte();
        combined_source.push_str(&doc.rope.byte_slice(start..end).to_string());
        combined_source.push('\n');
    }

    let valid_schema = ctx
        .schema
        .clone()
        .validate()
        .map_err(|e| format!("Invalid Schema: {}", e))?;

    let executable =
        executable::ExecutableDocument::parse(&valid_schema, &combined_source, "codegen.graphql")
            .map_err(|e| format!("GraphQL Parse Error: {}", e))?;

    for fragment in executable.fragments.values() {
        let type_name = format!("{}Fragment", fragment.name);
        body.push_str(&format!("export type {} = ", type_name));
        let parent_type = ctx
            .schema
            .types
            .get(fragment.type_condition().as_str())
            .ok_or_else(|| format!("Unknown type: {}", fragment.type_condition()))?;
        body.push_str(&generate_selection_set(
            &fragment.selection_set,
            parent_type,
            ctx,
            0,
            &mut used_fragments,
        ));
        body.push_str(";\n\n");
    }

    for operation in executable.operations.iter() {
        let op_name = operation
            .name
            .as_ref()
            .map(|n| n.as_str())
            .unwrap_or("UnnamedOperation");

        // Generate Variables Type
        if !operation.variables.is_empty() {
            body.push_str(&format!("export type {}Variables = {{\n", op_name));
            for var in &operation.variables {
                let ts_type = gql_type_to_ts(&var.ty, ctx.schema);
                let optional = if var.ty.is_non_null() { "" } else { "?" };
                body.push_str(&format!("  {}{}: {};\n", var.name, optional, ts_type));
            }
            body.push_str("};\n\n");
        } else {
            body.push_str(&format!(
                "export type {}Variables = Record<string, never>;\n\n",
                op_name
            ));
        }

        // Generate Result Type
        body.push_str(&format!("export type {} = ", op_name));
        let root_operation_name = ctx
            .schema
            .root_operation(operation.operation_type)
            .ok_or_else(|| format!("Schema does not support {:?}", operation.operation_type))?;
        let root_type = ctx
            .schema
            .types
            .get(root_operation_name.as_str())
            .ok_or_else(|| format!("Unknown root type: {}", root_operation_name))?;

        body.push_str(&generate_selection_set(
            &operation.selection_set,
            root_type,
            ctx,
            0,
            &mut used_fragments,
        ));
        body.push_str(";\n\n");
    }

    let mut output = String::new();
    output.push_str("/* tslint:disable */\n/* eslint-disable */\n// This file was automatically generated and should not be edited.\n\n");

    // Generate Imports
    let mut sorted_fragments: Vec<_> = used_fragments.iter().collect();
    sorted_fragments.sort();

    for frag_name in sorted_fragments {
        // Skip fragments defined in the same file
        if executable.fragments.contains_key(frag_name) {
            continue;
        }

        if let Some(source_path) = ctx.fragment_to_path.get(&frag_name.to_string()) {
            let source_path = Path::new(source_path);
            if let Some(rel_path) =
                pathdiff::diff_paths(source_path, ctx.current_file_path.parent().unwrap())
            {
                let mut path_str = rel_path.to_string_lossy().to_string();
                if !path_str.starts_with('.') {
                    path_str = format!("./{}", path_str);
                }
                // Strip extension for TS imports
                if let Some(idx) = path_str.rfind('.') {
                    path_str.truncate(idx);
                }
                // Handle the .codegen part
                if !path_str.ends_with(".codegen") {
                    path_str.push_str(".codegen");
                }

                output.push_str(&format!(
                    "import type {{ {}Fragment }} from \"{}\";\n",
                    frag_name, path_str
                ));
            }
        }
    }
    if !output.ends_with("\n\n") {
        output.push('\n');
    }
    output.push_str(&body);

    Ok(output)
}

fn generate_selection_set(
    selection_set: &executable::SelectionSet,
    parent_type: &apollo_compiler::schema::ExtendedType,
    ctx: &CodegenContext,
    indent: usize,
    used_fragments: &mut HashSet<apollo_compiler::Name>,
) -> String {
    let mut parts = Vec::new();
    let mut local_fields = String::new();
    let mut inline_fragments = Vec::new();
    let pad = "  ".repeat(indent + 1);

    let mut has_explicit_typename = false;
    for selection in &selection_set.selections {
        if let executable::Selection::Field(field) = selection {
            if field.name.as_str() == "__typename" && field.alias.is_none() {
                has_explicit_typename = true;
                break;
            }
        }
    }

    if !has_explicit_typename {
        let typename_value = match parent_type {
            apollo_compiler::schema::ExtendedType::Object(o) => format!("\"{}\"", o.name),
            apollo_compiler::schema::ExtendedType::Interface(i) => {
                let implementers = ctx.schema.implementers_map();
                if let Some(impls) = implementers.get(&i.name) {
                    let mut names: Vec<_> =
                        impls.objects.iter().map(|n| format!("\"{}\"", n)).collect();
                    names.sort();
                    names.join(" | ")
                } else {
                    "string".to_string()
                }
            }
            apollo_compiler::schema::ExtendedType::Union(u) => {
                let mut names: Vec<_> = u.members.iter().map(|n| format!("\"{}\"", n)).collect();
                names.sort();
                names.join(" | ")
            }
            _ => "string".to_string(),
        };
        local_fields.push_str(&format!("\n{}{}: {};", pad, "__typename", typename_value));
    }

    for selection in &selection_set.selections {
        match selection {
            executable::Selection::Field(field) => {
                let name = field
                    .alias
                    .as_ref()
                    .map(|a| a.as_str())
                    .unwrap_or(field.name.as_str());

                let field_def = match parent_type {
                    apollo_compiler::schema::ExtendedType::Object(obj) => {
                        obj.fields.get(field.name.as_str())
                    }
                    apollo_compiler::schema::ExtendedType::Interface(iface) => {
                        iface.fields.get(field.name.as_str())
                    }
                    _ => None,
                };

                if let Some(fd) = field_def {
                    let deprecation = fd.directives.get("deprecated").map(|d| {
                        d.argument_by_name("reason", ctx.schema)
                            .ok()
                            .and_then(|v| v.as_str())
                            .unwrap_or("No reason provided")
                    });
                    let doc_comment =
                        format_jsdoc(fd.description.as_deref(), deprecation, indent + 1);

                    let ts_type = if field.selection_set.selections.is_empty() {
                        gql_type_to_ts(&fd.ty, ctx.schema)
                    } else {
                        let inner_type_name = fd.ty.inner_named_type();
                        let inner_type = ctx
                            .schema
                            .types
                            .get(inner_type_name.as_str())
                            .expect("Field type must exist");
                        let base_type = generate_selection_set(
                            &field.selection_set,
                            inner_type,
                            ctx,
                            indent + 1,
                            used_fragments,
                        );
                        wrap_in_list_and_nullability(&base_type, &fd.ty)
                    };
                    local_fields
                        .push_str(&format!("\n{}{}{}: {};", doc_comment, pad, name, ts_type));
                } else if field.name.as_str() == "__typename" {
                    local_fields.push_str(&format!(
                        "\n{}{}: \"{}\";",
                        pad,
                        name,
                        parent_type.name()
                    ));
                }
            }
            executable::Selection::FragmentSpread(spread) => {
                parts.push(format!("{}Fragment", spread.fragment_name));
                used_fragments.insert(spread.fragment_name.clone());
            }
            executable::Selection::InlineFragment(inline) => {
                inline_fragments.push(inline);
            }
        }
    }

    let mut result_type = if !local_fields.is_empty() {
        let mut s = String::from("{");
        s.push_str(&local_fields);
        s.push_str(&format!("\n{}", "  ".repeat(indent)));
        s.push('}');
        s
    } else {
        String::new()
    };

    if !parts.is_empty() {
        let spreads = parts.join(" & ");
        if result_type.is_empty() {
            result_type = spreads;
        } else {
            result_type = format!("({} & {})", result_type, spreads);
        }
    }

    if !inline_fragments.is_empty() {
        let mut variants = Vec::new();
        for inline in inline_fragments {
            let target_type = if let Some(type_cond) = &inline.type_condition {
                ctx.schema
                    .types
                    .get(type_cond.as_str())
                    .expect("Type condition must exist")
            } else {
                parent_type
            };
            let inline_type = generate_selection_set(
                &inline.selection_set,
                target_type,
                ctx,
                indent + 1,
                used_fragments,
            );
            variants.push(inline_type);
        }

        let union_type = variants.join(" | ");
        if result_type.is_empty() {
            result_type = union_type;
        } else {
            // For Interface/Union, we intersect the shared fields (local_fields + spreads)
            // with the union of variants.
            result_type = format!("({} & ({}))", result_type, union_type);
        }
    }

    if result_type.is_empty() {
        "{}".to_string()
    } else {
        result_type
    }
}

pub fn generate_schema_types(schema: &Schema) -> String {
    let mut output = String::new();
    output.push_str("/* tslint:disable */\n/* eslint-disable */\n// This file was automatically generated and should not be edited.\n\n");

    // 1. Enums
    for (name, ty) in &schema.types {
        if name.starts_with("__") {
            continue;
        }
        if let apollo_compiler::schema::ExtendedType::Enum(enm) = ty {
            output.push_str(&format_jsdoc(enm.description.as_deref(), None, 0));
            let mut values: Vec<_> = enm.values.keys().collect();
            values.sort();
            let union_values = values
                .iter()
                .map(|v| format!("\"{}\"", v))
                .collect::<Vec<_>>()
                .join(" | ");
            output.push_str(&format!("export type {} = {};\n\n", name, union_values));
        }
    }

    // 2. Input Objects
    for (name, ty) in &schema.types {
        if name.starts_with("__") {
            continue;
        }
        if let apollo_compiler::schema::ExtendedType::InputObject(input) = ty {
            output.push_str(&format_jsdoc(input.description.as_deref(), None, 0));
            output.push_str(&format!("export interface {} {{\n", name));
            for field in input.fields.values() {
                let deprecation = field.directives.get("deprecated").map(|d| {
                    d.argument_by_name("reason", schema)
                        .ok()
                        .and_then(|v| v.as_str())
                        .unwrap_or("No reason provided")
                });
                output.push_str(&format_jsdoc(field.description.as_deref(), deprecation, 1));
                let ts_type = gql_type_to_ts_with_names(&field.ty, schema);
                let optional = if field.ty.is_non_null() { "" } else { "?" };
                output.push_str(&format!("  {}{}: {};\n", field.name, optional, ts_type));
            }
            output.push_str("}\n\n");
        }
    }

    // 3. Custom Scalars (Fallback to any if not handled in gql_type_to_ts)
    for (name, ty) in &schema.types {
        if let apollo_compiler::schema::ExtendedType::Scalar(scalar) = ty {
            match name.as_str() {
                "String" | "ID" | "Int" | "Float" | "Boolean" => continue,
                _ => {
                    output.push_str(&format_jsdoc(scalar.description.as_deref(), None, 0));
                    output.push_str(&format!("export type {} = any;\n\n", name));
                }
            }
        }
    }

    output
}

fn format_jsdoc(
    description: Option<&str>,
    deprecation_reason: Option<&str>,
    indent_level: usize,
) -> String {
    let has_desc = description.map_or(false, |d| !d.trim().is_empty());
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

fn generate_ts_type(ty: &apollo_compiler::ast::Type, base: &str) -> String {
    match ty {
        apollo_compiler::ast::Type::Named(_) => {
            if ty.is_non_null() {
                base.to_string()
            } else {
                format!("{} | null", base)
            }
        }
        apollo_compiler::ast::Type::List(inner) => {
            let inner_ts = generate_ts_type(inner, base);
            let list_ts = format!("Array<{}>", inner_ts);
            if ty.is_non_null() {
                list_ts
            } else {
                format!("{} | null", list_ts)
            }
        }
        apollo_compiler::ast::Type::NonNullNamed(_) => base.to_string(),
        apollo_compiler::ast::Type::NonNullList(inner) => {
            let inner_ts = generate_ts_type(inner, base);
            format!("Array<{}>", inner_ts)
        }
    }
}

fn gql_type_to_ts(ty: &apollo_compiler::ast::Type, schema: &Schema) -> String {
    gql_type_to_ts_internal(ty, schema, false)
}

fn gql_type_to_ts_with_names(ty: &apollo_compiler::ast::Type, schema: &Schema) -> String {
    gql_type_to_ts_internal(ty, schema, true)
}

fn gql_type_to_ts_internal(
    ty: &apollo_compiler::ast::Type,
    schema: &Schema,
    use_names: bool,
) -> String {
    let inner_name = ty.inner_named_type();
    let base = match inner_name.as_str() {
        "String" | "ID" => "string".to_string(),
        "Int" | "Float" => "number".to_string(),
        "Boolean" => "boolean".to_string(),
        other => {
            if let Some(t) = schema.types.get(other) {
                match t {
                    apollo_compiler::schema::ExtendedType::Enum(enm) => {
                        if use_names {
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
                    apollo_compiler::schema::ExtendedType::InputObject(_) => {
                        if use_names {
                            other.to_string()
                        } else {
                            "any".to_string()
                        }
                    }
                    apollo_compiler::schema::ExtendedType::Scalar(_) => {
                        if use_names {
                            other.to_string()
                        } else {
                            "any".to_string()
                        }
                    }
                    _ => "any".to_string(),
                }
            } else {
                "any".to_string()
            }
        }
    };

    generate_ts_type(ty, &base)
}

fn wrap_in_list_and_nullability(base: &str, ty: &apollo_compiler::ast::Type) -> String {
    generate_ts_type(ty, base)
}
