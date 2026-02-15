use ahash::AHashMap as HashMap;
use ahash::AHashSet as HashSet;
use apollo_compiler::ast::OperationType;
use graphox_core::apollo_ast::{serialize_fragment_definition, serialize_operation_definition};
use graphox_core::document::DocumentState;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::apply_naming_convention;
use crate::context::{CodegenContext, CodegenProfile, FragmentGenerated, OperationGenerated};
use crate::helpers::{get_fragment_deps_cached, get_operation_deps_cached, gql_type_to_ts};
use crate::selection_set::generate_selection_set;

use std::time::Instant;

struct DocumentAstInfo {
    raw_name: String,
    document_name: String,
    export_content: String,
    dependencies: HashSet<Arc<str>>,
    is_fragment: bool,
}

pub fn generate_typescript(
    doc: &DocumentState,
    ctx: &CodegenContext,
) -> Result<(String, Vec<OperationGenerated>, Vec<FragmentGenerated>), String> {
    let mut profile = CodegenProfile::default();
    let (output, ops, frags, _) = generate_typescript_with_profile(doc, ctx, &mut profile)?;
    Ok((output, ops, frags))
}

pub fn generate_typescript_with_profile(
    doc: &DocumentState,
    ctx: &CodegenContext,
    profile: &mut CodegenProfile,
) -> Result<
    (
        String,
        Vec<OperationGenerated>,
        Vec<FragmentGenerated>,
        CodegenProfile,
    ),
    String,
> {
    let mut output = String::with_capacity(4096);
    output.push_str("/* tslint:disable */\n/* eslint-disable */\n// This file was automatically generated and should not be edited.\n\ntype Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;\n\n");
    let mut generated_operations = Vec::new();
    let mut generated_fragments = Vec::new();
    let _import_start = std::time::Instant::now();

    let mut bodies = String::with_capacity(2048);
    let mut used_fragments = HashMap::default();
    let mut document_asts: Vec<DocumentAstInfo> = Vec::new();
    let mut has_fragment_asts = false;
    let mut has_operations = false;

    for block in doc.get_graphql_trees() {
        let block_text = graphox_core::utils::normalize_line_endings(
            &doc.rope
                .byte_slice(block.offset..(block.offset + block.tree.root_node().end_byte()))
                .to_string(),
        );

        let parse_start = Instant::now();
        let (exec_doc, errors) = match doc.get_executable_doc(ctx.schema, block.offset, &block_text)
        {
            Ok(d) => d,
            Err(e) if e == "SCHEMA_DEFINITION" => continue,
            Err(e) => return Err(e),
        };

        if let Some(errors) = errors {
            return Err(format!(
                "GraphQL validation errors in file {}:\n{}",
                doc.uri.path(),
                errors.join("\n")
            ));
        }

        profile.parse_time += parse_start.elapsed();

        for op in exec_doc.operations.iter() {
            has_operations = true;
            let raw_name = op
                .name
                .as_ref()
                .map(|n: &apollo_compiler::Name| n.as_str())
                .unwrap_or("UnnamedOperation");
            let name = apply_naming_convention(raw_name, &ctx.naming_convention());
            let suffix = match op.operation_type {
                OperationType::Query => ctx.query_suffix(),
                OperationType::Mutation => ctx.mutation_suffix(),
                OperationType::Subscription => ctx.subscription_suffix(),
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
                if !bodies.is_empty() {
                    bodies.push('\n');
                }
                bodies.push_str("export type ");
                bodies.push_str(&name);
                bodies.push_str(suffix);
                bodies.push_str(" = ");
                bodies.push_str(&result.type_str);
                bodies.push_str(";\n");
            } else {
                if !bodies.is_empty() {
                    bodies.push('\n');
                }
                bodies.push_str("export interface ");
                bodies.push_str(&name);
                bodies.push_str(suffix);
                bodies.push(' ');
                bodies.push_str(&result.type_str);
                bodies.push('\n');
            }

            let v_name = format!("{}{}{}", name, suffix, ctx.variables_suffix());
            bodies.push('\n');
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
            bodies.push_str("}>;\n");
            let vars_type = v_name.clone();

            let ast_start = Instant::now();
            let op_deps = if ctx.config.inline_fragments() {
                HashSet::default()
            } else {
                get_operation_deps_cached(op, ctx)
            };

            let op_direct_deps = if ctx.generate_ast_for_fragments && !ctx.config.inline_fragments()
            {
                let mut direct = HashSet::default();
                crate::helpers::collect_direct_fragment_spreads(&op.selection_set, &mut direct);
                direct
            } else {
                HashSet::default()
            };

            let ast_content = if ctx.generate_ast_for_fragments {
                let op_def = serialize_operation_definition(op, ctx.all_fragments, ctx.config);
                let deps = &op_direct_deps;

                let mut definitions_parts = Vec::with_capacity(deps.len() + 1);
                definitions_parts.push(op_def.to_string());

                let mut deps_list: Vec<_> = deps.iter().cloned().collect();
                deps_list.sort_unstable();

                for dep in deps_list {
                    if dep.as_ref() == raw_name {
                        continue;
                    }

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
                        let mut spread = String::with_capacity(dep.len() + 23);
                        spread.push_str("...");
                        let dep_name =
                            apply_naming_convention(dep.as_ref(), &ctx.naming_convention());
                        spread.push_str(&dep_name);
                        spread.push_str(ctx.fragment_suffix());
                        spread.push_str(ctx.fragment_document_suffix());
                        spread.push_str(".definitions");
                        definitions_parts.push(spread);
                    }
                }

                let mut result = String::new();
                result.push_str("{ kind: 'Document', definitions: [");
                result.push_str(&definitions_parts.join(", "));
                result.push_str("] }");
                result
            } else {
                graphox_core::apollo_ast::serialize_operation(op, ctx.all_fragments, ctx.config)
                    .to_string()
            };
            profile.ast_serialization_time += ast_start.elapsed();

            let doc_name = format!("{}{}{}", name, suffix, ctx.document_suffix());
            let mut export = String::new();
            export.push_str("export const ");
            export.push_str(&doc_name);
            export.push_str(" = ");
            export.push_str(&ast_content);
            export.push_str(" as unknown as DocumentNode<");
            export.push_str(&name);
            export.push_str(suffix);
            export.push_str(", ");
            export.push_str(&vars_type);
            export.push_str(">;\n");

            document_asts.push(DocumentAstInfo {
                raw_name: raw_name.to_string(),
                document_name: doc_name.clone(),
                export_content: export,
                dependencies: op_deps,
                is_fragment: false,
            });
            generated_operations.push(OperationGenerated {
                name: raw_name.to_string(),
                operation_type_name: format!("{}{}", name, suffix),
                variables_type_name: vars_type,
                document_name: doc_name,
                source_text: block_text.clone(),
                codegen_path: ctx.current_file_path.to_path_buf(),
            });
        }

        for frag in exec_doc.fragments.values() {
            let raw_name = frag.name.as_str();
            let name = apply_naming_convention(raw_name, &ctx.naming_convention());
            let fragment_type_name = format!("{}{}", name, ctx.fragment_suffix());
            let fragment_document_name = format!(
                "{}{}{}",
                name,
                ctx.fragment_suffix(),
                ctx.fragment_document_suffix()
            );

            let is_type_only = ctx
                .fragment_to_type_only
                .get(raw_name)
                .copied()
                .unwrap_or(false);

            let mut used_fragments_inner = HashMap::default();
            let root_type = ctx
                .schema
                .types
                .get(frag.type_condition().as_str())
                .ok_or_else(|| {
                    format!(
                        "Type condition {} not found",
                        frag.type_condition().as_str()
                    )
                })?;

            let sel_start = Instant::now();
            let result = generate_selection_set(
                &frag.selection_set,
                root_type,
                ctx,
                0,
                &mut used_fragments_inner,
            );
            profile.selection_set_time += sel_start.elapsed();

            if !bodies.is_empty() {
                bodies.push('\n');
            }
            if ctx.fragment_masking().is_enabled() {
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
                bodies.push_str("' };\n");
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

            let mut doc_export = String::new();
            let mut doc_deps = HashSet::default();

            if ctx.generate_ast_for_fragments && !is_type_only {
                has_fragment_asts = true;
                let ast_start = Instant::now();
                let frag_def = serialize_fragment_definition(frag, ctx.all_fragments, ctx.config);

                let deps = if ctx.config.inline_fragments() {
                    HashSet::default()
                } else {
                    let mut direct = HashSet::default();
                    crate::helpers::collect_direct_fragment_spreads(
                        &frag.selection_set,
                        &mut direct,
                    );
                    direct
                };

                doc_deps = if ctx.config.inline_fragments() {
                    HashSet::default()
                } else {
                    get_fragment_deps_cached(&frag.name, ctx)
                };

                let mut definitions_parts = Vec::with_capacity(deps.len() + 1);
                definitions_parts.push(frag_def.to_string());

                let mut deps_list: Vec<_> = deps.iter().collect();
                deps_list.sort_unstable();

                for dep in deps_list {
                    if dep.as_ref() == raw_name {
                        continue;
                    }

                    let is_dep_type_only = ctx
                        .fragment_to_type_only
                        .get(dep.as_ref())
                        .copied()
                        .unwrap_or_else(|| {
                            ctx.all_fragments
                                .get(dep.as_ref())
                                .map(|f| {
                                    ctx.fragment_to_type_only
                                        .get(f.name.as_str())
                                        .copied()
                                        .unwrap_or(false)
                                })
                                .unwrap_or(false)
                        });

                    if !is_dep_type_only {
                        let mut spread = String::with_capacity(dep.len() + 23);
                        spread.push_str("...");
                        let dep_name =
                            apply_naming_convention(dep.as_ref(), &ctx.naming_convention());
                        spread.push_str(&dep_name);
                        spread.push_str(ctx.fragment_suffix());
                        spread.push_str(ctx.fragment_document_suffix());
                        spread.push_str(".definitions");
                        definitions_parts.push(spread);
                    }
                }

                let mut ast_content = String::new();
                ast_content.push_str("{ kind: 'Document', definitions: [");
                ast_content.push_str(&definitions_parts.join(", "));
                ast_content.push_str("] }");

                if ctx.fragment_masking().is_enabled() {
                    doc_export.push_str("export const ");
                    doc_export.push_str(&fragment_document_name);
                    doc_export.push_str(" = ");
                    doc_export.push_str(&ast_content);
                    doc_export.push_str(" as unknown as DocumentNode<");
                    doc_export.push_str(&fragment_type_name);
                    doc_export.push_str(", unknown> & {\n");
                    doc_export.push_str("  __fragment: ");
                    doc_export.push_str(&fragment_type_name);
                    doc_export.push_str(";\n};\n");
                } else {
                    doc_export.push_str("export const ");
                    doc_export.push_str(&fragment_document_name);
                    doc_export.push_str(" = ");
                    doc_export.push_str(&ast_content);
                    doc_export.push_str(" as unknown as DocumentNode<");
                    doc_export.push_str(&fragment_type_name);
                    doc_export.push_str(", unknown>;\n");
                }
                profile.ast_serialization_time += ast_start.elapsed();
            } else if ctx.fragment_masking().is_enabled() {
                let frag_def = serialize_fragment_definition(frag, ctx.all_fragments, ctx.config);
                doc_export.push_str("export const ");
                doc_export.push_str(&fragment_document_name);
                doc_export.push_str(" = { kind: 'Document', definitions: [");
                doc_export.push_str(&frag_def.to_string());
                doc_export.push_str("] } as unknown as DocumentNode<");
                doc_export.push_str(&fragment_type_name);
                doc_export.push_str(", unknown> & {\n");
                doc_export.push_str("  __fragment: ");
                doc_export.push_str(&fragment_type_name);
                doc_export.push_str(";\n};\n");
            }

            if !doc_export.is_empty() {
                document_asts.push(DocumentAstInfo {
                    raw_name: raw_name.to_string(),
                    document_name: fragment_document_name.clone(),
                    export_content: doc_export,
                    dependencies: doc_deps,
                    is_fragment: true,
                });
            }

            generated_fragments.push(FragmentGenerated {
                name: raw_name.to_string(),
                fragment_type_name,
                source_text: block_text.clone(),
                document_name: fragment_document_name,
                codegen_path: ctx.current_file_path.to_path_buf(),
            });
        }
    }

    let mut import_section = String::new();
    if has_operations || has_fragment_asts || ctx.fragment_masking().is_enabled() {
        import_section.push_str("import type { TypedDocumentNode as DocumentNode } from \"@graphql-typed-document-node/core\";\n");
    }

    if ctx.fragment_masking().is_enabled() {
        import_section.push_str(&format!(
            "import type {{ FragmentType }} from \"{}\";\n",
            ctx.masking_import_path
        ));
    }

    let mut used_frag_names: Vec<_> = used_fragments.keys().cloned().collect();
    used_frag_names.sort_unstable();

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
        }
    }

    for (path, names) in &imports {
        let final_import_path = if ctx.fragment_to_import.values().any(|v| v == path) {
            path.to_string()
        } else {
            let rel_path =
                pathdiff::diff_paths(path.as_ref(), ctx.current_file_path.parent().unwrap())
                    .unwrap_or_else(|| Path::new(path.as_ref()).to_path_buf());
            let mut path_str = graphox_core::utils::to_posix_path(&rel_path);
            if !path_str.starts_with('.') {
                path_str.insert_str(0, "./");
            }
            let p = Path::new(&path_str);
            let stem = p.file_stem().unwrap().to_str().unwrap();
            let parent = p.parent().unwrap();
            let final_p = parent.join(stem);
            let mut final_path_str = graphox_core::utils::to_posix_path(&final_p);
            if !final_path_str.starts_with('.') && !final_path_str.starts_with('/') {
                final_path_str.insert_str(0, "./");
            }
            final_path_str.push_str(".codegen");
            final_path_str.push_str(ctx.emit_extensions().as_str());
            final_path_str
        };

        let mut names_to_import = names.clone();
        names_to_import.sort_unstable();

        let mut type_imports = Vec::new();
        let mut doc_imports = Vec::new();

        for n in names_to_import {
            let is_type_only = ctx
                .fragment_to_type_only
                .get(n.as_ref())
                .copied()
                .unwrap_or(false);
            let name = apply_naming_convention(n.as_ref(), &ctx.naming_convention());
            type_imports.push(format!("{}{}", name, ctx.fragment_suffix()));
            if ctx.generate_ast_for_fragments && !is_type_only {
                doc_imports.push(format!(
                    "{}{}{}",
                    name,
                    ctx.fragment_suffix(),
                    ctx.fragment_document_suffix()
                ));
            }
        }

        if ctx.fragment_masking().is_enabled() {
            import_section.push_str(&format!(
                "import {{ {} }} from \"{}\";\n",
                type_imports.join(", "),
                final_import_path
            ));
        } else {
            import_section.push_str(&format!(
                "import type {{ {} }} from \"{}\";\n",
                type_imports.join(", "),
                final_import_path
            ));
        }

        if !doc_imports.is_empty() {
            import_section.push_str(&format!(
                "import {{ {} }} from \"{}\";\n",
                doc_imports.join(", "),
                final_import_path
            ));
        }
    }

    {
        let used_schema_types = ctx.used_schema_types.borrow();
        if !used_schema_types.is_empty() {
            let mut grouped_imports: BTreeMap<String, Vec<String>> = BTreeMap::new();
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
                }
            }
            for (import_path, mut types) in grouped_imports {
                types.sort();
                import_section.push_str(&format!(
                    "import type {{ {} }} from \"{}\";\n",
                    types.join(", "),
                    import_path
                ));
            }
        }
    }

    output.push_str(&import_section);

    if has_operations {
        output.push('\n');
        output.push_str("export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };\n\n");
    }

    output.push_str(&bodies);
    profile.import_generation_time = _import_start.elapsed();

    if !document_asts.is_empty() {
        let sorted_docs = topological_sort_documents(&document_asts);
        for doc_info in sorted_docs {
            output.push_str(&doc_info.export_content);
        }
    }

    if bodies.is_empty() && document_asts.is_empty() {
        return Err("No executable operations or fragments found in this file".to_string());
    }

    Ok((
        output,
        generated_operations,
        generated_fragments,
        profile.clone(),
    ))
}

fn topological_sort_documents(docs: &[DocumentAstInfo]) -> Vec<&DocumentAstInfo> {
    use std::collections::VecDeque;
    let mut frag_name_to_doc_name = HashMap::default();
    for doc in docs {
        if doc.is_fragment {
            frag_name_to_doc_name.insert(doc.raw_name.as_str(), doc.document_name.as_str());
        }
    }
    let doc_names: HashSet<&str> = docs.iter().map(|d| d.document_name.as_str()).collect();
    let mut in_degree: HashMap<&str, usize> = HashMap::default();
    let mut graph: HashMap<&str, Vec<&str>> = HashMap::default();
    for doc in docs {
        let name = doc.document_name.as_str();
        in_degree.entry(name).or_insert(0);
        graph.entry(name).or_default();
        for dep in &doc.dependencies {
            if let Some(dep_doc_name) = frag_name_to_doc_name.get(dep.as_ref())
                && *dep_doc_name != name
                && doc_names.contains(dep_doc_name)
            {
                graph.entry(dep_doc_name).or_default().push(name);
                *in_degree.entry(name).or_insert(0) += 1;
            }
        }
    }
    let mut initial_nodes: Vec<&str> = in_degree
        .iter()
        .filter(|&(_, &d)| d == 0)
        .map(|(&n, _)| n)
        .collect();
    initial_nodes.sort_unstable();
    let mut queue: VecDeque<&str> = initial_nodes.into_iter().collect();
    let mut sorted_names = Vec::with_capacity(docs.len());
    while let Some(name) = queue.pop_front() {
        sorted_names.push(name);
        if let Some(dependents) = graph.get(name) {
            let mut sorted_dependents = dependents.clone();
            sorted_dependents.sort_unstable();
            for dependent in sorted_dependents {
                if let Some(degree) = in_degree.get_mut(dependent) {
                    *degree -= 1;
                    if *degree == 0 {
                        queue.push_back(dependent);
                    }
                }
            }
        }
    }
    let mut remaining: Vec<&DocumentAstInfo> = docs
        .iter()
        .filter(|d| !sorted_names.contains(&d.document_name.as_str()))
        .collect();
    remaining.sort_by_key(|d| d.document_name.as_str());
    let name_to_doc: HashMap<&str, &DocumentAstInfo> =
        docs.iter().map(|d| (d.document_name.as_str(), d)).collect();
    let mut result = Vec::with_capacity(docs.len());
    for name in sorted_names {
        if let Some(doc) = name_to_doc.get(name) {
            result.push(*doc);
        }
    }
    result.extend(remaining);
    result
}
