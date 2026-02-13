use ahash::AHashMap as HashMap;
use apollo_compiler::ast::OperationType;
use apollo_compiler::executable;
use graphox_core::apollo_ast::{serialize_fragment_definition, serialize_operation_definition};
use graphox_core::document::DocumentState;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::apply_naming_convention;
use crate::context::{CodegenContext, CodegenProfile, FragmentGenerated, OperationGenerated};
use crate::helpers::{get_fragment_deps_cached, get_operation_deps_cached, gql_type_to_ts};
use crate::selection_set::generate_selection_set;

pub fn generate_typescript(
    doc: &DocumentState,
    ctx: &CodegenContext,
) -> Result<(String, Vec<OperationGenerated>, Vec<FragmentGenerated>), String> {
    generate_typescript_with_profile(doc, ctx).map(|(s, ops, frags, _)| (s, ops, frags))
}

pub fn generate_typescript_with_profile(
    doc: &DocumentState,
    ctx: &CodegenContext,
) -> Result<
    (
        String,
        Vec<OperationGenerated>,
        Vec<FragmentGenerated>,
        CodegenProfile,
    ),
    String,
> {
    use std::time::Instant;
    let mut profile = CodegenProfile::default();

    // Pre-allocate output with estimated capacity
    let mut output = String::with_capacity(4096);
    output.push_str("/* tslint:disable */\n/* eslint-disable */\n// This file was automatically generated and should not be edited.\n\n");
    output.push_str("type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;\n\n");

    let mut used_fragments = HashMap::default();
    let mut generated_operations = Vec::new();
    let mut generated_fragments = Vec::new();

    // Pre-allocate bodies string
    let mut bodies = String::with_capacity(2048);
    let mut has_operations = false;
    let mut has_fragment_asts = false;

    for block in doc.get_graphql_trees() {
        // Avoid intermediate string allocation by using byte_slice directly
        let block_text = graphox_core::utils::normalize_line_endings(
            &doc.rope
                .byte_slice(block.offset..(block.offset + block.tree.root_node().end_byte()))
                .to_string(),
        );

        let parse_start = Instant::now();
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
        profile.parse_time += parse_start.elapsed();

        if !exec_doc.operations.is_empty() {
            has_operations = true;
        }

        for op in exec_doc.operations.iter() {
            let raw_name = op
                .name
                .as_ref()
                .map(|n| n.as_str())
                .unwrap_or("UnnamedOperation");
            let name = apply_naming_convention(raw_name, &ctx.naming_convention);
            let suffix = match op.operation_type {
                OperationType::Query => ctx.query_suffix,
                OperationType::Mutation => ctx.mutation_suffix,
                OperationType::Subscription => ctx.subscription_suffix,
            };

            let root_type = ctx
                .schema
                .root_operation(op.operation_type)
                .and_then(|n| ctx.schema.types.get(n.as_str()))
                .ok_or_else(|| format!("Root type for {:?} not found", op.operation_type))?;

            let sel_start = Instant::now();
            let result =
                generate_selection_set(&op.selection_set, root_type, ctx, 0, &mut used_fragments);
            profile.selection_set_time += sel_start.elapsed();

            if result.needs_type_declaration {
                bodies.push_str("export type ");
                bodies.push_str(&name);
                bodies.push_str(suffix);
                bodies.push_str(" = ");
                bodies.push_str(&result.type_str);
                bodies.push_str(";\n\n");
            } else {
                bodies.push_str("export interface ");
                bodies.push_str(&name);
                bodies.push_str(suffix);
                bodies.push(' ');
                bodies.push_str(&result.type_str);
                bodies.push_str("\n\n");
            }

            let v_name = format!("{}{}{}", name, suffix, ctx.variables_suffix);
            bodies.push_str("export type ");
            bodies.push_str(&v_name);
            bodies.push_str(" = Exact<{\n");
            if !op.variables.is_empty() {
                for var in &op.variables {
                    let ts_type_str = gql_type_to_ts(&var.ty, ctx.schema, ctx.scalars, ctx);
                    let optional = if var.ty.is_non_null() { "" } else { "?" };
                    bodies.push_str("  ");
                    bodies.push_str(&var.name);
                    bodies.push_str(optional);
                    bodies.push_str(": ");
                    bodies.push_str(&ts_type_str);
                    bodies.push_str(";\n");
                }
            }
            bodies.push_str("}>;\n\n");
            let vars_type = v_name.clone();

            let ast_start = Instant::now();
            let ast_content = if ctx.generate_ast_for_fragments {
                let op_def =
                    serialize_operation_definition(op, ctx.all_fragments, &ctx.ast_emit_config);

                // Use cached dependencies when possible to avoid expensive tree traversal
                let deps = get_operation_deps_cached(op, ctx, doc);

                // Pre-allocate with known capacity to avoid reallocations
                let estimated_size = deps.len();
                let mut definitions_parts = Vec::with_capacity(estimated_size + 1);
                let op_def_str = op_def.to_string();
                definitions_parts.push(op_def_str);

                // Sort deps once to avoid repeated allocations
                let mut deps_list: Vec<_> = deps.into_iter().collect();
                deps_list.sort_unstable(); // unstable sort is faster

                for dep in deps_list {
                    let is_type_only = ctx
                        .fragment_to_type_only
                        .get(dep.as_ref())
                        .copied()
                        .unwrap_or_else(|| {
                            doc.fragments()
                                .iter()
                                .find(|f| f.name.as_ref() == dep.as_ref())
                                .map(|f| f.is_type_only)
                                .unwrap_or(false)
                        });

                    if !is_type_only {
                        // Use direct string building instead of format! macro
                        let mut spread = String::with_capacity(dep.len() + 23); // "...Document.definitions"
                        spread.push_str("...");
                        spread.push_str(&dep);
                        spread.push_str(ctx.document_suffix);
                        spread.push_str(".definitions");
                        definitions_parts.push(spread);
                    }
                }

                // Pre-calculate total size for final string
                let total_size: usize = definitions_parts.iter().map(|s| s.len()).sum::<usize>()
                    + definitions_parts.len() * 2
                    + 30;
                let mut result = String::with_capacity(total_size);
                result.push_str("{ kind: 'Document', definitions: [");
                result.push_str(&definitions_parts.join(", "));
                result.push_str("] }");
                result
            } else {
                graphox_core::apollo_ast::serialize_operation(
                    op,
                    ctx.all_fragments,
                    &ctx.ast_emit_config,
                )
                .to_string()
            };
            profile.ast_serialization_time += ast_start.elapsed();

            let doc_name = format!("{}{}", name, suffix);
            bodies.push_str("export const ");
            bodies.push_str(&doc_name);
            bodies.push_str(ctx.document_suffix);
            bodies.push_str(" = ");
            bodies.push_str(&ast_content);
            bodies.push_str(" as unknown as DocumentNode<");
            bodies.push_str(&name);
            bodies.push_str(suffix);
            bodies.push_str(", ");
            bodies.push_str(&vars_type);
            bodies.push_str(">;\n\n");

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

            let sel_start = Instant::now();
            let result =
                generate_selection_set(&frag.selection_set, type_def, ctx, 0, &mut used_fragments);
            profile.selection_set_time += sel_start.elapsed();

            let fragment_type_name = format!(
                "{}{}",
                apply_naming_convention(frag.name.as_str(), &ctx.naming_convention),
                ctx.fragment_suffix
            );
            let fragment_document_name =
                format!("{}{}", fragment_type_name, ctx.fragment_document_suffix);

            if ctx.fragment_masking.is_enabled() {
                let type_str = if result.type_str.contains('|') && !result.type_str.starts_with('(')
                {
                    format!("({})", result.type_str.trim())
                } else {
                    result.type_str.trim().to_string()
                };

                bodies.push_str("export type ");
                bodies.push_str(&fragment_type_name);
                bodies.push_str(" = ");
                bodies.push_str(&type_str);
                bodies.push_str(" & { ' $fragmentName'?: '");
                bodies.push_str(&fragment_type_name);
                bodies.push_str("' };\n\n");

                bodies.push_str("export declare const ");
                bodies.push_str(&fragment_document_name);
                bodies.push_str(": {\n");
                bodies.push_str("  __fragment: ");
                bodies.push_str(&fragment_type_name);
                bodies.push_str(";\n};\n\n");
            } else if result.needs_type_declaration {
                bodies.push_str("export type ");
                bodies.push_str(&fragment_type_name);
                bodies.push_str(" = ");
                bodies.push_str(&result.type_str);
                bodies.push_str(";\n");
            } else {
                bodies.push_str("export interface ");
                bodies.push_str(&fragment_type_name);
                bodies.push(' ');
                bodies.push_str(&result.type_str);
                bodies.push('\n');
            }

            if ctx.generate_ast_for_fragments {
                let ast_start = Instant::now();
                let is_type_only = doc
                    .fragments()
                    .iter()
                    .find(|f| f.name.as_ref() == frag.name.as_str())
                    .map(|f| f.is_type_only)
                    .unwrap_or(false);

                if !is_type_only {
                    has_fragment_asts = true;
                    let frag_def = serialize_fragment_definition(
                        frag,
                        ctx.all_fragments,
                        &ctx.ast_emit_config,
                    );

                    // Use cached dependencies to avoid tree traversal
                    let deps = get_fragment_deps_cached(&frag.name, ctx);

                    // Pre-allocate with known capacity
                    let estimated_size = deps.len();
                    let mut definitions_parts = Vec::with_capacity(estimated_size + 1);
                    let frag_def_str = frag_def.to_string();
                    definitions_parts.push(frag_def_str);

                    // Sort deps once
                    let mut deps_list: Vec<_> = deps.into_iter().collect();
                    deps_list.sort_unstable();

                    for dep in deps_list {
                        let is_dep_type_only = ctx
                            .fragment_to_type_only
                            .get(dep.as_ref())
                            .copied()
                            .unwrap_or_else(|| {
                                doc.fragments()
                                    .iter()
                                    .find(|f| f.name.as_ref() == dep.as_ref())
                                    .map(|f| f.is_type_only)
                                    .unwrap_or(false)
                            });

                        if !is_dep_type_only {
                            // Use direct string building
                            let mut spread = String::with_capacity(dep.len() + 23);
                            spread.push_str("...");
                            spread.push_str(&dep);
                            spread.push_str(ctx.document_suffix);
                            spread.push_str(".definitions");
                            definitions_parts.push(spread);
                        }
                    }

                    // Pre-calculate total size
                    let total_size: usize =
                        definitions_parts.iter().map(|s| s.len()).sum::<usize>()
                            + definitions_parts.len() * 2;
                    let mut definitions = String::with_capacity(total_size);
                    definitions.push('[');
                    definitions.push_str(&definitions_parts.join(", "));
                    definitions.push(']');

                    bodies.push_str("export const ");
                    bodies.push_str(&fragment_document_name);
                    bodies.push_str(" = { kind: 'Document', definitions: ");
                    bodies.push_str(&definitions);
                    bodies.push_str(" } as unknown as DocumentNode<");
                    bodies.push_str(&fragment_type_name);
                    bodies.push_str(", unknown>;\n");
                }
                profile.ast_serialization_time += ast_start.elapsed();
            }

            bodies.push('\n');

            generated_fragments.push(FragmentGenerated {
                name: fragment_type_name.clone(),
                source_text: block_text.clone(),
                document_name: fragment_document_name,
                codegen_path: ctx.codegen_path.clone(),
            });
        }
    }

    // Add imports for fragments used from other files
    let import_start = Instant::now();
    let mut used_frag_names: Vec<_> = used_fragments.keys().cloned().collect();
    used_frag_names.sort_unstable();

    // Use BTreeMap to keep imports sorted, avoiding need to sort later
    let mut imports: BTreeMap<Arc<str>, Vec<Arc<str>>> = BTreeMap::new();
    let current_path = doc.uri.path();
    for frag_name in used_frag_names {
        if let Some(import_alias) = ctx.fragment_to_import.get(&frag_name[..]) {
            imports
                .entry(import_alias.clone())
                .or_default()
                .push(frag_name.clone().into());
        } else if let Some(other_path) = ctx.fragment_to_path.get(&frag_name[..]) {
            let other_path_buf = PathBuf::from(other_path.as_ref());
            let current_path_buf = PathBuf::from(current_path);
            let other_canonical = std::fs::canonicalize(&other_path_buf).unwrap_or(other_path_buf);
            let current_canonical =
                std::fs::canonicalize(&current_path_buf).unwrap_or(current_path_buf);
            if other_canonical != current_canonical {
                imports
                    .entry(other_path.clone())
                    .or_default()
                    .push(frag_name.clone().into());
            }
        } else {
            return Err(format!(
                "Fragment '{}' not found in current project and is not marked as @public in other projects",
                frag_name
            ));
        }
    }

    let mut import_section = String::new();

    {
        let used_schema_types = ctx.used_schema_types.borrow();
        if !used_schema_types.is_empty() {
            let mut grouped_imports: BTreeMap<String, Vec<String>> = BTreeMap::new();
            let mut untracked_types = Vec::new();

            for ty in used_schema_types.iter() {
                if let Some(import_path) = ctx.type_imports.get(ty) {
                    grouped_imports
                        .entry(import_path.clone())
                        .or_default()
                        .push(ty.clone());
                } else if let Some(import_path) = ctx.schema_import {
                    grouped_imports
                        .entry(import_path.clone())
                        .or_default()
                        .push(ty.clone());
                } else {
                    untracked_types.push(ty.clone());
                }
            }

            for (import_path, mut types) in grouped_imports {
                types.sort();
                let mut line = String::new();
                line.push_str("import type { ");
                line.push_str(&types.join(", "));
                line.push_str(" } from \"");
                line.push_str(&import_path);
                line.push_str("\";\n");
                import_section.push_str(&line);
            }

            if !untracked_types.is_empty()
                && ctx.schema_import.is_none()
                && ctx.type_imports.is_empty()
            {
                // If no imports configured but types used, we might want to warn or just skip
                // For now, we follow existing behavior which was to not emit imports if schema_import is None
            }
        }
    }

    // Pre-compute current_file_parent to avoid repeated calls
    let current_file_parent = ctx.current_file_path.parent().unwrap();

    // BTreeMap iteration is already sorted, no need to sort
    for (path, names) in &imports {
        let final_import_path = if ctx.fragment_to_import.values().any(|v| v == path) {
            // It's an alias
            path.to_string()
        } else {
            // It's a file path, need to relativize
            let rel_path = pathdiff::diff_paths(path.as_ref(), current_file_parent)
                .unwrap_or_else(|| Path::new(path.as_ref()).to_path_buf());
            let mut path_str = graphox_core::utils::to_posix_path(&rel_path);
            if !path_str.starts_with('.') {
                path_str.insert_str(0, "./");
            }
            // Change extension to .codegen - optimized string building
            let p = Path::new(&path_str);
            let stem = p.file_stem().unwrap().to_str().unwrap();
            let parent = p.parent().unwrap();
            let final_p = parent.join(stem);
            let mut final_path_str = graphox_core::utils::to_posix_path(&final_p);
            if !final_path_str.starts_with('.') && !final_path_str.starts_with('/') {
                final_path_str.insert_str(0, "./");
            }
            final_path_str.push_str(".codegen");
            final_path_str.push_str(ctx.emit_extensions.as_str());
            final_path_str
        };

        let suffixed_names: Vec<_> = names
            .iter()
            .map(|n| format!("{}{}", n, ctx.fragment_suffix))
            .collect();

        if ctx.fragment_masking.is_enabled() {
            import_section.push_str("import { ");
        } else {
            import_section.push_str("import type { ");
        }
        import_section.push_str(&suffixed_names.join(", "));
        import_section.push_str(" } from \"");
        import_section.push_str(&final_import_path);
        import_section.push_str("\";\n");

        if ctx.generate_ast_for_fragments {
            let mut doc_names = Vec::new();
            for name in names {
                let is_type_only = ctx
                    .fragment_to_type_only
                    .get(&name[..])
                    .copied()
                    .unwrap_or_else(|| {
                        doc.fragments()
                            .iter()
                            .find(|f| f.name.as_ref() == name.as_ref())
                            .map(|f| f.is_type_only)
                            .unwrap_or(false)
                    });

                if !is_type_only {
                    let mut doc_name =
                        String::with_capacity(name.len() + ctx.document_suffix.len());
                    doc_name.push_str(name);
                    doc_name.push_str(ctx.document_suffix);
                    doc_names.push(doc_name);
                }
            }

            if !doc_names.is_empty() {
                import_section.push_str("import { ");
                import_section.push_str(&doc_names.join(", "));
                import_section.push_str(" } from \"");
                import_section.push_str(&final_import_path);
                import_section.push_str("\";\n");
            }
        }
    }

    if has_operations || has_fragment_asts {
        output.push_str("import type { TypedDocumentNode as DocumentNode } from \"@graphql-typed-document-node/core\";\n");
    }

    if ctx.fragment_masking.is_enabled() {
        output.push_str(&format!(
            "import type {{ FragmentType }} from \"{}\";\n",
            ctx.masking_import_path
        ));
    }

    if !import_section.is_empty() {
        output.push_str(&import_section);
    }

    if has_operations || !import_section.is_empty() {
        output.push('\n');
    }

    if has_operations {
        output.push_str("export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };\n\n");
    }

    output.push_str(&bodies);
    profile.import_generation_time = import_start.elapsed();

    if bodies.is_empty() {
        return Err("No executable operations or fragments found in this file".to_string());
    }

    Ok((output, generated_operations, generated_fragments, profile))
}
