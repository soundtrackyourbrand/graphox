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

    let mut bodies = String::with_capacity(2048);
    let mut used_fragments = HashSet::default();
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

            if !bodies.is_empty() {
                bodies.push('\n');
            }
            if result.needs_type_declaration {
                bodies.push_str("export type ");
                bodies.push_str(&name);
                bodies.push_str(suffix);
                bodies.push_str(" = ");
                bodies.push_str(&result.type_str);
                bodies.push_str(";\n");
            } else {
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

                let mut result = String::with_capacity(definitions_parts.len() * 100 + 40);
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
            let result =
                generate_selection_set(&frag.selection_set, root_type, ctx, 0, &mut used_fragments);
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

                let ast_content = if let Some(cached) = ctx.get_fragment_ast(&Arc::from(raw_name)) {
                    cached.to_string()
                } else {
                    let frag_def =
                        serialize_fragment_definition(frag, ctx.all_fragments, ctx.config);

                    let mut deps = get_fragment_deps_cached(&frag.name, ctx);
                    if deps.is_empty() {
                        deps = graphox_core::apollo_ast::get_fragment_fragment_dependencies(
                            frag,
                            ctx.all_fragments,
                        );
                    }
                    doc_deps = deps.clone();

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

                    let mut content = String::with_capacity(definitions_parts.len() * 100 + 40);
                    content.push_str("{ kind: 'Document', definitions: [");
                    content.push_str(&definitions_parts.join(", "));
                    content.push_str("] }");

                    let content_arc: Arc<str> = content.into();
                    ctx.insert_fragment_ast(Arc::from(raw_name), content_arc.clone());
                    content_arc.to_string()
                };

                doc_export.push_str("export const ");
                doc_export.push_str(&fragment_document_name);
                doc_export.push_str(" = ");
                doc_export.push_str(&ast_content);
                doc_export.push_str(" as unknown as DocumentNode<");
                doc_export.push_str(&fragment_type_name);
                doc_export.push_str(", unknown>");

                if ctx.fragment_masking().is_enabled() {
                    doc_export.push_str(" & {\n");
                    doc_export.push_str("  __fragment: ");
                    doc_export.push_str(&fragment_type_name);
                    doc_export.push_str(";\n};\n");
                } else {
                    doc_export.push_str(";\n");
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

    let import_start = Instant::now();
    let mut import_section = String::new();
    if has_operations || has_fragment_asts || ctx.fragment_masking().is_enabled() {
        import_section.push_str("import type { TypedDocumentNode as DocumentNode } from \"@graphql-typed-document-node/core\";\n");
    }

    if ctx.fragment_masking().is_enabled() {
        import_section.push_str("import type { FragmentType } from \"");
        import_section.push_str(&ctx.masking_import_path);
        import_section.push_str("\";\n");
    }

    let mut final_used_fragments = HashSet::default();
    let mut to_expand: Vec<Arc<str>> = used_fragments.iter().cloned().collect();

    while let Some(frag_name) = to_expand.pop() {
        if final_used_fragments.insert(frag_name.clone()) {
            if let Some(deps) = ctx.fragment_dependencies.get(frag_name.as_ref()) {
                for dep in deps {
                    to_expand.push(dep.clone());
                }
            } else if let Some(frag) = ctx.all_fragments.get(frag_name.as_ref()) {
                let deps = graphox_core::apollo_ast::get_fragment_fragment_dependencies(
                    frag,
                    ctx.all_fragments,
                );
                for dep in deps {
                    to_expand.push(dep);
                }
            }
        }
    }
    *used_fragments = final_used_fragments.into();

    let mut used_frag_names: Vec<_> = used_fragments.iter().cloned().collect();
    used_frag_names.sort_unstable();

    let mut imports: BTreeMap<Arc<str>, Vec<Arc<str>>> = BTreeMap::new();
    let current_path = doc
        .uri
        .to_file_path()
        .unwrap_or_else(|_| PathBuf::from(doc.uri.path()));
    let current_canonical = ctx.canonicalize_path(&current_path);

    for frag_name in used_frag_names {
        if let Some(import_alias) = ctx.fragment_to_import.get(&frag_name[..]) {
            imports
                .entry(import_alias.clone())
                .or_default()
                .push(frag_name);
        } else if let Some(other_path) = ctx.fragment_to_path.get(&frag_name[..]) {
            let other_path_buf = Path::new(other_path.as_ref());
            let other_canonical = ctx.canonicalize_path(other_path_buf);

            if other_canonical != current_canonical {
                imports
                    .entry(other_path.clone())
                    .or_default()
                    .push(frag_name);
            }
        }
    }

    for (path, names) in &imports {
        let final_import_path = if ctx.fragment_to_import.values().any(|v| v == path) {
            path.to_string()
        } else {
            ctx.get_final_import_path(path, ctx.current_file_path.parent().unwrap())
        };

        let mut type_imports = Vec::with_capacity(names.len());
        let mut doc_imports = Vec::with_capacity(names.len());

        for n in names {
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
            import_section.push_str("import { ");
        } else {
            import_section.push_str("import type { ");
        }
        import_section.push_str(&type_imports.join(", "));
        import_section.push_str(" } from \"");
        import_section.push_str(&final_import_path);
        import_section.push_str("\";\n");

        if !doc_imports.is_empty() {
            import_section.push_str("import { ");
            import_section.push_str(&doc_imports.join(", "));
            import_section.push_str(" } from \"");
            import_section.push_str(&final_import_path);
            import_section.push_str("\";\n");
        }
    }

    {
        let used_schema_types = ctx.used_schema_types.borrow();
        if !used_schema_types.is_empty() {
            let mut grouped_imports: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
            for ty in used_schema_types.iter() {
                if let Some(import_path) = ctx.type_imports.get(ty) {
                    grouped_imports
                        .entry(import_path.as_str())
                        .or_default()
                        .push(ty.as_str());
                } else if let Some(import_path) = ctx.schema_import.as_deref() {
                    grouped_imports
                        .entry(import_path)
                        .or_default()
                        .push(ty.as_str());
                }
            }
            for (import_path, mut types) in grouped_imports {
                types.sort_unstable();
                import_section.push_str("import type { ");
                import_section.push_str(&types.join(", "));
                import_section.push_str(" } from \"");
                import_section.push_str(import_path);
                import_section.push_str("\";\n");
            }
        }
    }

    output.push_str(&import_section);
    profile.import_generation_time = import_start.elapsed();

    if has_operations {
        output.push('\n');
        output.push_str("export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };\n\n");
    }

    output.push_str(&bodies);

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
