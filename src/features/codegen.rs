use crate::features::apollo_ast::{
    get_fragment_fragment_dependencies, get_operation_fragment_dependencies,
    serialize_fragment_definition, serialize_operation_definition,
};
use apollo_compiler::ast::{OperationType, Type};
use apollo_compiler::executable::{self, Selection, SelectionSet};
use apollo_compiler::schema::ExtendedType;
use apollo_compiler::{Node, Schema};
use fnv::{FnvHashMap as HashMap, FnvHashSet as HashSet};
use std::path::{Path, PathBuf};

pub struct CodegenContext<'a> {
    pub schema: &'a apollo_compiler::validation::Valid<Schema>,
    pub fragment_to_path: &'a HashMap<String, String>,
    pub fragment_to_import: &'a HashMap<String, String>,
    pub all_fragments: &'a HashMap<String, Node<executable::Fragment>>,
    pub current_file_path: &'a Path,
    pub scalars: &'a Option<HashMap<String, String>>,
    pub schema_import: &'a Option<String>,
    pub generate_ast_for_fragments: bool,
}

pub struct OperationGenerated {
    pub name: String,
    pub source_text: String,
    pub operation_type_name: String, // e.g. GetMeQuery
    pub variables_type_name: String, // e.g. GetMeQueryVariables
    pub codegen_path: PathBuf,       // Path to the .codegen.ts file
}

pub fn generate_typescript(
    doc: &crate::DocumentState,
    ctx: &CodegenContext,
) -> Result<(String, Vec<OperationGenerated>), String> {
    let mut output = String::new();
    output.push_str("/* tslint:disable */\n/* eslint-disable */\n// This file was automatically generated and should not be edited.\n\n");

    let mut used_fragments = HashMap::default();
    let mut generated_operations = Vec::new();

    let mut bodies = String::new();
    let mut has_operations = false;
    let mut used_schema_types = HashSet::default();

    for block in doc.get_graphql_trees() {
        let block_text = doc
            .rope
            .byte_slice(block.offset..(block.offset + block.tree.root_node().end_byte()))
            .to_string();

        let exec_doc =
            match executable::ExecutableDocument::parse(ctx.schema, &block_text, "doc.graphql") {
                Ok(d) => d,
                Err(e) => {
                    let error_str = e.to_string();
                    if error_str.contains("must not contain") {
                        // It's a schema definition, skip it
                        continue;
                    }
                    // It's a real error in an executable doc
                    return Err(format!("Failed to parse GraphQL block: {}", e));
                }
            };

        if !exec_doc.operations.is_empty() {
            has_operations = true;
        }

        for op in exec_doc.operations.iter() {
            let name = op
                .name
                .as_ref()
                .map(|n| n.as_str())
                .unwrap_or("UnnamedOperation");
            let suffix = match op.operation_type {
                OperationType::Query => "Query",
                OperationType::Mutation => "Mutation",
                OperationType::Subscription => "Subscription",
            };

            let root_type = ctx
                .schema
                .root_operation(op.operation_type)
                .and_then(|n| ctx.schema.types.get(n.as_str()))
                .ok_or_else(|| format!("Root type for {:?} not found", op.operation_type))?;

            let ts_type = generate_selection_set(
                &op.selection_set,
                root_type,
                ctx,
                0,
                &mut used_fragments,
                &mut used_schema_types,
            );

            bodies.push_str(&format!(
                "export interface {}{} {}\n\n",
                name, suffix, ts_type
            ));

            let vars_type = if !op.variables.is_empty() {
                let v_name = format!("{}{}Variables", name, suffix);
                bodies.push_str(&format!("export interface {} {{\n", v_name));
                for var in &op.variables {
                    let ts_type_str = gql_type_to_ts(
                        &var.ty,
                        ctx.schema,
                        ctx.scalars,
                        ctx,
                        &mut used_schema_types,
                    );
                    let optional = if var.ty.is_non_null() { "" } else { "?" };
                    bodies.push_str(&format!("  {}{}: {};\n", var.name, optional, ts_type_str));
                }
                bodies.push_str("}\n\n");
                v_name
            } else {
                "{ [key: string]: never; }".to_string()
            };

            let ast_content = if ctx.generate_ast_for_fragments {
                let op_def = serialize_operation_definition(op);
                let deps = get_operation_fragment_dependencies(op, ctx.all_fragments);
                let mut deps_list: Vec<_> = deps.into_iter().collect();
                deps_list.sort();

                let mut definitions_parts = vec![op_def.to_string()];
                for dep in deps_list {
                    definitions_parts.push(format!("...{}Document.definitions", dep));
                }

                let definitions = format!("[{}]", definitions_parts.join(", "));

                format!("{{ kind: 'Document', definitions: {} }}", definitions)
            } else {
                crate::features::apollo_ast::serialize_operation(op, ctx.all_fragments).to_string()
            };

            bodies.push_str(&format!(
                "export const {}Document = {} as unknown as DocumentNode<{}{}, {}>;\n\n",
                name, ast_content, name, suffix, vars_type
            ));

            generated_operations.push(OperationGenerated {
                name: name.to_string(),
                source_text: block_text.clone(),
                operation_type_name: format!("{}{}", name, suffix),
                variables_type_name: vars_type,
                codegen_path: ctx.current_file_path.to_path_buf(), // Placeholder
            });
        }

        for frag in exec_doc.fragments.values() {
            let type_name = frag.type_condition().as_str();
            let type_def = ctx
                .schema
                .types
                .get(type_name)
                .ok_or_else(|| format!("Type {} not found in schema", type_name))?;

            let ts_type = generate_selection_set(
                &frag.selection_set,
                type_def,
                ctx,
                0,
                &mut used_fragments,
                &mut used_schema_types,
            );
            bodies.push_str(&format!("export interface {} {}\n\n", frag.name, ts_type));

            if ctx.generate_ast_for_fragments {
                let frag_def = serialize_fragment_definition(frag);
                let deps = get_fragment_fragment_dependencies(frag, ctx.all_fragments);
                let mut deps_list: Vec<_> = deps.into_iter().collect();
                deps_list.sort();

                let mut definitions_parts = vec![frag_def.to_string()];
                for dep in deps_list {
                    definitions_parts.push(format!("...{}Document.definitions", dep));
                }

                let definitions = format!("[{}]", definitions_parts.join(", "));

                bodies.push_str(&format!(
                    "export const {}Document = {{ kind: 'Document', definitions: {} }} as unknown as DocumentNode<any, any>;\n\n",
                    frag.name, definitions
                ));
            }
        }
    }

    // Add imports for fragments used from other files
    let mut used_frag_names: Vec<_> = used_fragments.keys().cloned().collect();
    used_frag_names.sort();

    let mut imports: HashMap<String, Vec<String>> = HashMap::default();
    let current_path = doc.uri.path();
    for frag_name in used_frag_names {
        if let Some(import_alias) = ctx.fragment_to_import.get(&frag_name) {
            imports
                .entry(import_alias.clone())
                .or_default()
                .push(frag_name);
        } else if let Some(other_path) = ctx.fragment_to_path.get(&frag_name) {
            // fragment_to_path contains absolute paths as strings.
            // doc.uri.path() is also absolute.
            if other_path != current_path {
                imports
                    .entry(other_path.clone())
                    .or_default()
                    .push(frag_name);
            }
        } else {
            return Err(format!(
                "Fragment '{}' not found in current project and is not marked as @public in other projects",
                frag_name
            ));
        }
    }

    let mut import_section = String::new();

    if let Some(schema_import_path) = ctx.schema_import
        && !used_schema_types.is_empty()
    {
        let mut types: Vec<_> = used_schema_types.into_iter().collect();
        types.sort();
        import_section.push_str(&format!(
            "import type {{ {} }} from \"{}\";\n",
            types.join(", "),
            schema_import_path
        ));
    }

    let mut import_paths: Vec<_> = imports.keys().cloned().collect();
    import_paths.sort();

    for path in import_paths {
        let names = imports.get(&path).unwrap();

        let final_import_path = if ctx.fragment_to_import.values().any(|v| v == &path) {
            // It's an alias
            path
        } else {
            // It's a file path, need to relativize
            let rel_path = pathdiff::diff_paths(&path, ctx.current_file_path.parent().unwrap())
                .unwrap_or_else(|| Path::new(&path).to_path_buf());
            let mut path_str = rel_path.to_string_lossy().to_string();
            if !path_str.starts_with('.') {
                path_str = format!("./{}", path_str);
            }
            // Change extension to .codegen
            let p = Path::new(&path_str);
            let stem = p.file_stem().unwrap().to_str().unwrap();
            let parent = p.parent().unwrap();
            let final_p = parent.join(stem);
            let mut final_path_str = final_p.to_string_lossy().to_string();
            if !final_path_str.starts_with('.') && !final_path_str.starts_with('/') {
                final_path_str = format!("./{}", final_path_str);
            }
            format!("{}.codegen", final_path_str)
        };

        import_section.push_str(&format!(
            "import type {{ {} }} from \"{}\";\n",
            names.join(", "),
            final_import_path
        ));

        if ctx.generate_ast_for_fragments {
            let doc_names: Vec<_> = names.iter().map(|n| format!("{}Document", n)).collect();
            import_section.push_str(&format!(
                "import {{ {} }} from \"{}\";\n",
                doc_names.join(", "),
                final_import_path
            ));
        }
    }

    if has_operations {
        output.push_str("import type { TypedDocumentNode as DocumentNode } from \"@graphql-typed-document-node/core\";\n");
    }

    if !import_section.is_empty() {
        output.push_str(&import_section);
    }

    if has_operations || !import_section.is_empty() {
        output.push('\n');
    }

    output.push_str(&bodies);

    if bodies.is_empty() {
        return Err("No executable operations or fragments found in this file".to_string());
    }

    Ok((output, generated_operations))
}

pub fn generate_entrypoint_content(output_dir: &Path, operations: &[OperationGenerated]) -> String {
    let mut output = String::new();
    output.push_str("/* tslint:disable */\n/* eslint-disable */\n// This file was automatically generated and should not be edited.\n\n");
    output.push_str("import type { TypedDocumentNode as DocumentNode } from \"@graphql-typed-document-node/core\";\n");

    let mut import_lines = Vec::new();
    let mut overloads = Vec::new();
    let mut map_entries = Vec::new();

    for op in operations {
        let rel_codegen_path = pathdiff::diff_paths(&op.codegen_path, output_dir)
            .unwrap_or_else(|| op.codegen_path.clone());
        let mut path_str = rel_codegen_path.to_string_lossy().to_string();
        if !path_str.starts_with('.') && !path_str.starts_with('/') {
            path_str = format!("./{}", path_str);
        }
        // Remove extension
        let path_no_ext = if path_str.ends_with(".ts") {
            &path_str[..path_str.len() - 3]
        } else {
            &path_str
        };

        import_lines.push(format!(
            "import {{ {}, {}Variables, {}Document }} from \"{}\";",
            op.operation_type_name, op.operation_type_name, op.operation_type_name, path_no_ext
        ));

        overloads.push(format!(
            "export function graphql(source: {:?}): typeof {}Document;",
            op.source_text, op.operation_type_name
        ));

        map_entries.push(format!(
            "  {:?}: {}Document,",
            op.source_text, op.operation_type_name
        ));
    }

    import_lines.sort();
    import_lines.dedup();
    for line in import_lines {
        output.push_str(&line);
        output.push('\n');
    }
    output.push('\n');

    let mut map_body = String::new();
    for entry in map_entries {
        map_body.push_str(&entry);
        map_body.push('\n');
    }

    output.push_str(&format!(
        "const documents: {{ [key: string]: any }} = {{\n{}}};\n\n",
        map_body
    ));

    for overload in overloads {
        output.push_str(&overload);
        output.push('\n');
    }
    output.push_str("export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;\n");
    output.push_str(
        "export function graphql(source: string): any {\n  return documents[source] || {};\n}\n\n",
    );
    output.push_str("export const gql = graphql;\n");

    output
}

pub fn generate_permissions_content(
    schema: &apollo_compiler::validation::Valid<Schema>,
    scalars: &Option<HashMap<String, String>>,
    schema_import: &Option<String>,
) -> String {
    let mut output = String::new();
    output.push_str("/* tslint:disable */\n/* eslint-disable */\n// This file was automatically generated and should not be edited.\n\n");

    let mut types_with_permissions = Vec::new();
    let mut names: Vec<_> = schema.types.keys().collect();
    names.sort();

    for name in names {
        if name.starts_with("__") {
            continue;
        }
        let ty = schema.types.get(name).unwrap();
        let fields = match ty {
            ExtendedType::Object(obj) => Some(&obj.fields),
            ExtendedType::Interface(iface) => Some(&iface.fields),
            _ => None,
        };

        if let Some(fields) = fields {
            if let Some(permissions_field) = fields.get("permissions") {
                let inner_name = permissions_field.ty.inner_named_type();
                let inner_type = schema.types.get(inner_name.as_str());
                if let Some(ExtendedType::Enum(_)) = inner_type {
                    types_with_permissions.push((name, permissions_field));
                } else {
                    eprintln!(
                        "Warning: Type '{}' has a 'permissions' field, but its type '{}' is not an enum. Skipping permissions generation for this type.",
                        name, inner_name
                    );
                }
            }
        }
    }

    if types_with_permissions.is_empty() {
        output.push_str("export interface PermissionsType {}\n\n");
        output.push_str("export const permissionTypes = {};\n");
        return output;
    }

    if let Some(import_path) = schema_import {
        let mut types_to_import = HashSet::default();
        for (_, field) in &types_with_permissions {
            let inner_name = field.ty.inner_named_type();
            types_to_import.insert(inner_name.to_string());
        }
        if !types_to_import.is_empty() {
            let mut sorted_imports: Vec<_> = types_to_import.into_iter().collect();
            sorted_imports.sort();
            output.push_str(&format!(
                "import type {{ {} }} from \"{}\";\n\n",
                sorted_imports.join(", "),
                import_path
            ));
        }
    }

    let empty_fragments = HashMap::default();
    let dummy_ctx = CodegenContext {
        schema,
        fragment_to_path: &HashMap::default(),
        fragment_to_import: &HashMap::default(),
        all_fragments: &empty_fragments,
        current_file_path: Path::new(""),
        scalars,
        schema_import,
        generate_ast_for_fragments: false,
    };
    let mut used_schema_types = HashSet::default();

    output.push_str("export interface PermissionsType {\n");
    for (typename, field) in &types_with_permissions {
        let ts_type = gql_type_to_ts_with_names(
            &field.ty,
            schema,
            scalars,
            &dummy_ctx,
            &mut used_schema_types,
        );
        output.push_str(&format!("  {}: {};\n", typename, ts_type));
    }
    output.push_str("}\n\n");

    output.push_str("export const permissionTypes = {\n");
    for (typename, field) in &types_with_permissions {
        let inner_name = field.ty.inner_named_type();
        if let Some(ExtendedType::Enum(enm)) = schema.types.get(inner_name.as_str()) {
            let mut values: Vec<_> = enm.values.keys().collect();
            values.sort();
            let values_str = values
                .iter()
                .map(|v| format!("'{}'", v))
                .collect::<Vec<_>>()
                .join(", ");
            output.push_str(&format!("  {}: [{}],\n", typename, values_str));
        }
    }
    output.push_str("}\n");

    output
}

fn generate_selection_set(
    selection_set: &SelectionSet,
    parent_type: &ExtendedType,
    ctx: &CodegenContext,
    indent: usize,
    used_fragments: &mut HashMap<String, String>,
    used_schema_types: &mut HashSet<String>,
) -> String {
    let pad = "  ".repeat(indent);
    let inner_pad = "  ".repeat(indent + 1);

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

    if inline_fragments.is_empty() {
        // Simple object or intersection
        let mut local_fields_list = Vec::new();
        if !has_explicit_typename {
            local_fields_list.push(format!("__typename: \"{}\"", parent_type.name()));
        }

        for field in fields {
            let name = field.alias.as_ref().unwrap_or(&field.name);
            let field_def = match parent_type {
                ExtendedType::Object(obj) => obj.fields.get(field.name.as_str()),
                ExtendedType::Interface(iface) => iface.fields.get(field.name.as_str()),
                _ => None,
            };

            if let Some(fd) = field_def {
                let ts_type = if field.selection_set.selections.is_empty() {
                    gql_type_to_ts(&fd.ty, ctx.schema, ctx.scalars, ctx, used_schema_types)
                } else {
                    let inner_type_name = fd.ty.inner_named_type();
                    let inner_type = ctx
                        .schema
                        .types
                        .get(inner_type_name.as_str())
                        .expect("Field type must exist");
                    generate_selection_set(
                        &field.selection_set,
                        inner_type,
                        ctx,
                        indent + 1,
                        used_fragments,
                        used_schema_types,
                    )
                };

                let wrapped_type = if field.selection_set.selections.is_empty() {
                    ts_type
                } else {
                    wrap_in_list_and_nullability(&ts_type, &fd.ty)
                };

                local_fields_list.push(format!("{}: {}", name, wrapped_type));
            }
        }

        if fragment_spreads.is_empty() {
            // Standard multi-line object (keep this for simple_query)
            let mut result = String::new();
            for f in local_fields_list {
                result.push_str(&format!("\n{}{};", inner_pad, f));
            }
            format!("{{{}\n{}}}", result, pad)
        } else {
            // Intersection mode (compact style for fragment_usage)
            let base_obj = format!("{{ {} }}", local_fields_list.join(", "));
            let mut spreads: Vec<_> = fragment_spreads
                .iter()
                .map(|s| s.fragment_name.as_str())
                .collect();
            spreads.sort();

            format!("({} & {})", base_obj, spreads.join(" & "))
        }
    } else {
        // Union type
        let mut branches = Vec::new();

        // Base branch
        let base_branch = format!("{{ __typename: \"{}\" }}", parent_type.name());
        branches.push(base_branch);

        for inline in inline_fragments {
            let type_name = inline
                .type_condition
                .as_ref()
                .map(|n| n.as_str())
                .unwrap_or_else(|| parent_type.name());
            let target_type = ctx.schema.types.get(type_name).unwrap_or(parent_type);

            let mut branch_fields = String::new();
            branch_fields.push_str(&format!("\n{}    __typename: \"{}\";", pad, type_name));

            // Generate fields for this fragment
            for selection in &inline.selection_set.selections {
                if let Selection::Field(field) = selection {
                    let name = field.alias.as_ref().unwrap_or(&field.name);
                    if name.as_str() == "__typename" {
                        continue;
                    }

                    let field_def = match target_type {
                        ExtendedType::Object(obj) => obj.fields.get(field.name.as_str()),
                        ExtendedType::Interface(iface) => iface.fields.get(field.name.as_str()),
                        _ => None,
                    };

                    if let Some(fd) = field_def {
                        let ts_type = if field.selection_set.selections.is_empty() {
                            gql_type_to_ts(&fd.ty, ctx.schema, ctx.scalars, ctx, used_schema_types)
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
                                indent + 2,
                                used_fragments,
                                used_schema_types,
                            );
                            wrap_in_list_and_nullability(&base_type, &fd.ty)
                        };
                        branch_fields.push_str(&format!("\n{}    {}: {};", pad, name, ts_type));
                    }
                }
            }
            branches.push(format!("{{{}\n{}  }}", branch_fields, pad));
        }

        for spread in fragment_spreads {
            branches.push(spread.fragment_name.to_string());
            used_fragments.insert(spread.fragment_name.to_string(), String::new());
        }

        let mut result = branches[0].clone();
        for (i, branch) in branches.iter().enumerate().skip(1) {
            if i == 1 {
                result.push_str(&format!("\n{}  | ", pad));
            } else {
                result.push_str(" | ");
            }
            result.push_str(branch);
        }
        result
    }
}

fn wrap_in_list_and_nullability(base: &str, ty: &Type) -> String {
    let mut result = base.to_string();
    if !ty.is_non_null() {
        result = format!("{} | null", result);
    }
    if ty.is_list() {
        result = format!("Array<{}>", result);
        if !ty.is_non_null() {
            result = format!("{} | null", result);
        }
    }
    result
}

fn gql_type_to_ts(
    ty: &Type,
    schema: &Schema,
    scalars: &Option<HashMap<String, String>>,
    ctx: &CodegenContext,
    used_schema_types: &mut HashSet<String>,
) -> String {
    gql_type_to_ts_internal(ty, schema, false, scalars, ctx, used_schema_types)
}

fn gql_type_to_ts_with_names(
    ty: &Type,
    schema: &Schema,
    scalars: &Option<HashMap<String, String>>,
    ctx: &CodegenContext,
    used_schema_types: &mut HashSet<String>,
) -> String {
    gql_type_to_ts_internal(ty, schema, true, scalars, ctx, used_schema_types)
}

fn gql_type_to_ts_internal(
    ty: &Type,
    schema: &Schema,
    use_names: bool,
    scalars: &Option<HashMap<String, String>>,
    ctx: &CodegenContext,
    used_schema_types: &mut HashSet<String>,
) -> String {
    let inner_name = ty.inner_named_type();
    let base = match inner_name.as_str() {
        "String" | "ID" => "string".to_string(),
        "Int" | "Float" => "number".to_string(),
        "Boolean" => "boolean".to_string(),
        other => {
            if let Some(config_scalars) = scalars
                && let Some(mapped) = config_scalars.get(other)
            {
                mapped.to_string()
            } else if let Some(t) = schema.types.get(other) {
                match t {
                    ExtendedType::Enum(enm) => {
                        if ctx.schema_import.is_some() {
                            used_schema_types.insert(other.to_string());
                            other.to_string()
                        } else if use_names {
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
                        if ctx.schema_import.is_some() {
                            used_schema_types.insert(other.to_string());
                            other.to_string()
                        } else if use_names {
                            other.to_string()
                        } else {
                            "any".to_string()
                        }
                    }
                    ExtendedType::Object(_)
                    | ExtendedType::Interface(_)
                    | ExtendedType::Union(_) => {
                        if ctx.schema_import.is_some() || use_names {
                            used_schema_types.insert(other.to_string());
                            other.to_string()
                        } else {
                            "any".to_string()
                        }
                    }
                }
            } else {
                "any".to_string()
            }
        }
    };

    generate_ts_type(ty, &base)
}

pub fn generate_schema_types(
    schema: &apollo_compiler::validation::Valid<Schema>,
    scalars: &Option<HashMap<String, String>>,
) -> String {
    let mut output = String::new();
    output.push_str("/* tslint:disable */\n/* eslint-disable */\n// This file was automatically generated and should not be edited.\n\n");

    let empty_fragments = HashMap::default();
    let dummy_ctx = CodegenContext {
        schema,
        fragment_to_path: &HashMap::default(),
        fragment_to_import: &HashMap::default(),
        all_fragments: &empty_fragments,
        current_file_path: Path::new(""),
        scalars,
        schema_import: &None,
        generate_ast_for_fragments: false,
    };
    let mut used_schema_types = HashSet::default();

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
                let ts_type = gql_type_to_ts_with_names(
                    &field.ty,
                    schema,
                    scalars,
                    &dummy_ctx,
                    &mut used_schema_types,
                );
                let optional = if field.ty.is_non_null() { "" } else { "?" };
                output.push_str(&format!("  {}{}: {};\n", field.name, optional, ts_type));
            }
            output.push_str("}\n\n");
        }
    }

    // 3. Custom Scalars (Fallback to any if not handled in gql_type_to_ts)
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

fn format_jsdoc(
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

fn generate_ts_type(ty: &Type, base: &str) -> String {
    let mut result = base.to_string();
    if ty.is_list() {
        result = format!("Array<{}>", result);
    }
    if !ty.is_non_null() {
        result = format!("{} | null", result);
    }
    result
}
