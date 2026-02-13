use ahash::AHashMap as HashMap;
use apollo_compiler::Schema;
use apollo_compiler::schema::ExtendedType;
use graphox_core::config::EmitExtensions;
use std::path::{Path, PathBuf};

use crate::context::{CodegenContext, FragmentMasking, TypeCache};
use crate::helpers::{format_jsdoc, get_interface_implementors, gql_type_to_ts_with_names};

pub fn generate_schema_types(
    schema: &apollo_compiler::validation::Valid<Schema>,
    scalars: &Option<HashMap<String, String>>,
) -> String {
    // Pre-allocate with larger capacity for schema types
    let mut output = String::with_capacity(8192);
    output.push_str("/* tslint:disable */\n/* eslint-disable */\n// This file was automatically generated and should not be edited.\n\n");

    let empty_fragments = HashMap::default();
    let empty_deps = HashMap::default();
    let empty_path_map = HashMap::default();
    let empty_import_map = HashMap::default();
    let empty_type_only_map = HashMap::default();
    let dummy_cache = TypeCache::new();
    let empty_type_imports = HashMap::default();
    let dummy_ctx = CodegenContext::new(
        schema,
        &empty_path_map,
        &empty_import_map,
        &empty_type_only_map,
        &empty_fragments,
        Path::new(""),
        scalars,
        &None,
        &empty_type_imports,
        false,
        &empty_deps,
        &dummy_cache,
        "Document",
        "Variables",
        "",
        "",
        "Query",
        "Mutation",
        "Subscription",
        graphox_core::config::NamingConvention::default(),
        FragmentMasking::Disabled,
        "./fragment-masking".to_string(),
        EmitExtensions::None,
        PathBuf::new(),
    );

    // 1. Enums
    let mut enum_names: Vec<_> = schema.types.keys().collect();
    enum_names.sort();

    for name in enum_names {
        if name.starts_with("__") {
            continue;
        }
        if let Some(ExtendedType::Enum(enm)) = schema.types.get(name) {
            let deprecation = enm.directives.get("deprecated").map(|d| {
                d.argument_by_name("reason", schema)
                    .ok()
                    .and_then(|v| v.as_str())
                    .unwrap_or("No reason provided")
            });
            output.push_str(&format_jsdoc(enm.description.as_deref(), deprecation, 0));
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
    let mut input_names: Vec<_> = schema.types.keys().collect();
    input_names.sort();

    for name in input_names {
        if name.starts_with("__") {
            continue;
        }
        if let Some(ExtendedType::InputObject(input)) = schema.types.get(name) {
            let deprecation = input.directives.get("deprecated").map(|d| {
                d.argument_by_name("reason", schema)
                    .ok()
                    .and_then(|v| v.as_str())
                    .unwrap_or("No reason provided")
            });
            output.push_str(&format_jsdoc(input.description.as_deref(), deprecation, 0));
            output.push_str(&format!("export interface {} {{\n", name));
            for field in input.fields.values() {
                let field_deprecation = field.directives.get("deprecated").map(|d| {
                    d.argument_by_name("reason", schema)
                        .ok()
                        .and_then(|v| v.as_str())
                        .unwrap_or("No reason provided")
                });
                output.push_str(&format_jsdoc(
                    field.description.as_deref(),
                    field_deprecation,
                    1,
                ));
                let ts_type = gql_type_to_ts_with_names(&field.ty, schema, scalars, &dummy_ctx);
                let optional = if field.ty.is_non_null() { "" } else { "?" };
                output.push_str(&format!("  {}{}: {};\n", field.name, optional, ts_type));
            }
            output.push_str("}\n\n");
        }
    }

    // 3. Objects
    let mut object_names: Vec<_> = schema.types.keys().collect();
    object_names.sort();

    for name in object_names {
        if name.starts_with("__") {
            continue;
        }
        if let Some(ExtendedType::Object(obj)) = schema.types.get(name) {
            let deprecation = obj.directives.get("deprecated").map(|d| {
                d.argument_by_name("reason", schema)
                    .ok()
                    .and_then(|v| v.as_str())
                    .unwrap_or("No reason provided")
            });
            output.push_str(&format_jsdoc(obj.description.as_deref(), deprecation, 0));
            output.push_str(&format!("export interface {} {{\n", name));
            output.push_str(&format!("  __typename: \"{}\";\n", name));

            let mut field_names: Vec<_> = obj.fields.keys().collect();
            field_names.sort();

            for field_name in field_names {
                let field = &obj.fields[field_name];
                let field_deprecation = field.directives.get("deprecated").map(|d| {
                    d.argument_by_name("reason", schema)
                        .ok()
                        .and_then(|v| v.as_str())
                        .unwrap_or("No reason provided")
                });
                output.push_str(&format_jsdoc(
                    field.description.as_deref(),
                    field_deprecation,
                    1,
                ));
                let ts_type = gql_type_to_ts_with_names(&field.ty, schema, scalars, &dummy_ctx);
                let optional = if field.ty.is_non_null() { "" } else { "?" };
                output.push_str(&format!("  {}{}: {};\n", field.name, optional, ts_type));
            }
            output.push_str("}\n\n");
        }
    }

    // 4. Interfaces
    let mut interface_names: Vec<_> = schema.types.keys().collect();
    interface_names.sort();

    for name in interface_names {
        if name.starts_with("__") {
            continue;
        }
        if let Some(ExtendedType::Interface(interface)) = schema.types.get(name) {
            let deprecation = interface.directives.get("deprecated").map(|d| {
                d.argument_by_name("reason", schema)
                    .ok()
                    .and_then(|v| v.as_str())
                    .unwrap_or("No reason provided")
            });
            output.push_str(&format_jsdoc(
                interface.description.as_deref(),
                deprecation,
                0,
            ));
            output.push_str(&format!("export interface {} {{\n", name));

            // For interfaces, __typename is a union of all possible types
            let mut implementors = get_interface_implementors(name, schema);
            implementors.sort();

            if !implementors.is_empty() {
                output.push_str(&format!("  __typename: {};\n", implementors.join(" | ")));
            } else {
                output.push_str("  __typename: string;\n");
            }

            let mut field_names: Vec<_> = interface.fields.keys().collect();
            field_names.sort();

            for field_name in field_names {
                let field = &interface.fields[field_name];
                let field_deprecation = field.directives.get("deprecated").map(|d| {
                    d.argument_by_name("reason", schema)
                        .ok()
                        .and_then(|v| v.as_str())
                        .unwrap_or("No reason provided")
                });
                output.push_str(&format_jsdoc(
                    field.description.as_deref(),
                    field_deprecation,
                    1,
                ));
                let ts_type = gql_type_to_ts_with_names(&field.ty, schema, scalars, &dummy_ctx);
                let optional = if field.ty.is_non_null() { "" } else { "?" };
                output.push_str(&format!("  {}{}: {};\n", field.name, optional, ts_type));
            }
            output.push_str("}\n\n");
        }
    }

    // 5. Unions
    let mut union_names: Vec<_> = schema.types.keys().collect();
    union_names.sort();

    for name in union_names {
        if name.starts_with("__") {
            continue;
        }
        if let Some(ExtendedType::Union(un)) = schema.types.get(name) {
            let deprecation = un.directives.get("deprecated").map(|d| {
                d.argument_by_name("reason", schema)
                    .ok()
                    .and_then(|v| v.as_str())
                    .unwrap_or("No reason provided")
            });
            output.push_str(&format_jsdoc(un.description.as_deref(), deprecation, 0));
            let mut members: Vec<_> = un.members.iter().map(|m| m.to_string()).collect();
            members.sort();
            output.push_str(&format!(
                "export type {} = {};\n\n",
                name,
                members.join(" | ")
            ));
        }
    }

    // 6. Custom Scalars (Fallback to any if not handled in gql_type_to_ts)
    let mut scalar_names: Vec<_> = schema.types.keys().collect();
    scalar_names.sort();

    for name in scalar_names {
        if let Some(ExtendedType::Scalar(scalar)) = schema.types.get(name) {
            match name.as_str() {
                "String" | "ID" | "Int" | "Float" | "Boolean" => continue,
                _ => {
                    let deprecation = scalar.directives.get("deprecated").map(|d| {
                        d.argument_by_name("reason", schema)
                            .ok()
                            .and_then(|v| v.as_str())
                            .unwrap_or("No reason provided")
                    });
                    output.push_str(&format_jsdoc(scalar.description.as_deref(), deprecation, 0));

                    let ts_type = if let Some(config_scalars) = scalars
                        && let Some(mapped) = config_scalars.get(name.as_str())
                    {
                        mapped.to_string()
                    } else {
                        "any".to_string()
                    };

                    output.push_str(&format!("export type {} = {};\n\n", name, ts_type));
                }
            }
        }
    }

    output
}
