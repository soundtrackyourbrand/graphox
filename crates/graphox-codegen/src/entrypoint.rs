use graphox_core::config::CodegenConfig;
use std::collections::BTreeMap;
use std::path::Path;

use crate::context::{FragmentGenerated, OperationGenerated};

#[allow(clippy::too_many_arguments)]
pub fn generate_entrypoint_content(
    output_dir: &Path,
    operations: &[OperationGenerated],
    fragments: &[FragmentGenerated],
    codegen_config: &CodegenConfig,
    re_exports: bool,
    schema_import: Option<&str>,
) -> String {
    let emit_extensions = codegen_config.emit_extensions();
    let generate_ast_for_fragments = codegen_config.generate_ast_for_fragments();
    let graphql_tag_fallback = codegen_config.graphql_tag_fallback();
    let op_count = operations.len();
    let frag_count = fragments.len();
    let estimated_size = (op_count + frag_count) * 200 + 500;
    let mut output = String::with_capacity(estimated_size);
    output.push_str("/* tslint:disable */\n/* eslint-disable */\n// This file was automatically generated and should not be edited.\n\n");

    if graphql_tag_fallback {
        output.push_str("import gqlTag from \"graphql-tag\";\n");
    }

    let mut path_cache: std::collections::HashMap<std::path::PathBuf, std::path::PathBuf> =
        std::collections::HashMap::new();
    let mut get_relative_path = |path: &Path| -> std::path::PathBuf {
        if let Some(cached) = path_cache.get(path) {
            return cached.clone();
        }
        let res = pathdiff::diff_paths(path, output_dir).unwrap_or_else(|| path.to_path_buf());
        path_cache.insert(path.to_path_buf(), res.clone());
        res
    };

    let ext = emit_extensions.as_str();
    output.push_str("import type { TypedDocumentNode as DocumentNode } from \"@graphql-typed-document-node/core\";\n");

    output.push_str("type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;\n");

    let mut type_import_lines = Vec::with_capacity(operations.len());
    let mut runtime_import_lines = Vec::with_capacity(operations.len());
    let mut overloads = String::with_capacity(operations.len() * 100);
    let mut map_entries = String::with_capacity(operations.len() * 80);

    // Deduplicate operations by source text to avoid duplicate overloads and map entries
    let mut unique_ops_by_source = BTreeMap::new();
    // Deduplicate operations by name to avoid duplicate imports
    let mut unique_ops_by_name = BTreeMap::new();

    for op in operations {
        unique_ops_by_source.entry(&op.source_text).or_insert(op);
        unique_ops_by_name
            .entry(&op.operation_type_name)
            .or_insert(op);
    }

    for op in unique_ops_by_name.values() {
        let rel_codegen_path = get_relative_path(&op.codegen_path);
        let mut path_str = graphox_core::utils::to_posix_path(&rel_codegen_path);
        if !path_str.starts_with('.') && !path_str.starts_with('/') {
            path_str = format!("./{}", path_str);
        }
        let path_no_ext = if path_str.ends_with(".ts") {
            &path_str[..path_str.len() - 3]
        } else {
            &path_str
        };
        let final_path = format!("{}{}", path_no_ext, ext);

        type_import_lines.push(format!(
            "import type {{ {}, {} }} from \"{}\";",
            op.operation_type_name, op.variables_type_name, final_path
        ));

        runtime_import_lines.push(format!(
            "import {{ {} }} from \"{}\";",
            op.document_name, final_path
        ));
    }

    for op in unique_ops_by_source.values() {
        overloads.push_str(&format!(
            "export function graphql(source: {:?}): typeof {};\n",
            op.source_text, op.document_name
        ));

        map_entries.push_str(&format!("  {:?}: {},\n", op.source_text, op.document_name));
    }

    let mut unique_frags_by_name = BTreeMap::new();

    for frag in fragments {
        unique_frags_by_name.entry(&frag.name).or_insert(frag);
    }

    // Track source texts already added (from operations) to avoid duplicate keys
    let mut added_source_texts: Vec<&String> = unique_ops_by_source.keys().cloned().collect();

    for frag in unique_frags_by_name.values() {
        let rel_codegen_path = get_relative_path(&frag.codegen_path);
        let mut path_str = graphox_core::utils::to_posix_path(&rel_codegen_path);
        if !path_str.starts_with('.') && !path_str.starts_with('/') {
            path_str = format!("./{}", path_str);
        }
        let path_no_ext = if path_str.ends_with(".ts") {
            &path_str[..path_str.len() - 3]
        } else {
            &path_str
        };
        let final_path = format!("{}{}", path_no_ext, ext);

        if generate_ast_for_fragments {
            runtime_import_lines.push(format!(
                "import {{ {} }} from \"{}\";",
                frag.document_name, final_path
            ));
        } else {
            type_import_lines.push(format!(
                "import type {{ {} }} from \"{}\";",
                frag.fragment_type_name, final_path
            ));
        }
    }

    for frag in unique_frags_by_name.values() {
        // Skip if this source text was already added (from an operation)
        if added_source_texts.contains(&&frag.source_text) {
            continue;
        }
        added_source_texts.push(&frag.source_text);

        if generate_ast_for_fragments {
            overloads.push_str(&format!(
                "export function graphql(source: {:?}): typeof {};\n",
                frag.source_text, frag.document_name
            ));

            map_entries.push_str(&format!(
                "  {:?}: {},\n",
                frag.source_text, frag.document_name
            ));
        } else {
            overloads.push_str(&format!(
                "export function graphql(source: {:?}): DocumentNode<{}, unknown>;\n",
                frag.source_text, frag.fragment_type_name
            ));

            map_entries.push_str(&format!("  {:?}: {{}},\n", frag.source_text));
        }
    }

    let mut all_import_lines: Vec<_> = type_import_lines
        .iter()
        .chain(runtime_import_lines.iter())
        .collect();
    all_import_lines.sort();
    all_import_lines.dedup();
    for line in all_import_lines {
        output.push_str(line);
        output.push('\n');
    }
    output.push('\n');

    output.push_str("const documents: { [key: string]: any } = {\n");
    output.push_str(&map_entries);
    output.push_str("};\n\n");

    output.push_str(&overloads);
    output.push_str("export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;\n");
    if graphql_tag_fallback {
        output.push_str(
            "export function graphql(source: string): any {\n  return documents[source] || (documents[source] = gqlTag(source));\n}\n\n",
        );
    } else {
        output.push_str(
            "export function graphql(source: string): any {\n  return documents[source] || {};\n}\n\n",
        );
    }
    output.push_str("export const gql = graphql;\n");

    if re_exports {
        output.push_str("\n// Re-exports\n");

        let mut path_to_ops: BTreeMap<String, Vec<&OperationGenerated>> = BTreeMap::new();
        for op in unique_ops_by_name.values() {
            let rel_codegen_path = get_relative_path(&op.codegen_path);
            let mut path_str = graphox_core::utils::to_posix_path(&rel_codegen_path);
            if !path_str.starts_with('.') && !path_str.starts_with('/') {
                path_str = format!("./{}", path_str);
            }
            let path_no_ext = if path_str.ends_with(".ts") {
                &path_str[..path_str.len() - 3]
            } else {
                &path_str
            };
            let final_path = format!("{}{}", path_no_ext, ext);
            path_to_ops.entry(final_path).or_default().push(op);
        }

        for (path, ops) in &path_to_ops {
            let mut types = Vec::new();
            let mut docs = Vec::new();
            for op in ops {
                types.push(op.operation_type_name.clone());
                types.push(op.variables_type_name.clone());
                docs.push(op.document_name.clone());
            }
            output.push_str(&format!(
                "export type {{ {} }} from \"{}\";\n",
                types.join(", "),
                path
            ));
            output.push_str(&format!(
                "export {{ {} }} from \"{}\";\n",
                docs.join(", "),
                path
            ));
        }

        let mut path_to_frags: BTreeMap<String, Vec<&FragmentGenerated>> = BTreeMap::new();
        for frag in unique_frags_by_name.values() {
            let rel_codegen_path = get_relative_path(&frag.codegen_path);
            let mut path_str = graphox_core::utils::to_posix_path(&rel_codegen_path);
            if !path_str.starts_with('.') && !path_str.starts_with('/') {
                path_str = format!("./{}", path_str);
            }
            let path_no_ext = if path_str.ends_with(".ts") {
                &path_str[..path_str.len() - 3]
            } else {
                &path_str
            };
            let final_path = format!("{}{}", path_no_ext, ext);
            path_to_frags.entry(final_path).or_default().push(frag);
        }

        for (path, frags) in &path_to_frags {
            let mut types = Vec::new();
            let mut docs = Vec::new();
            for frag in frags {
                types.push(frag.fragment_type_name.clone());
                if generate_ast_for_fragments {
                    docs.push(frag.document_name.clone());
                }
            }
            output.push_str(&format!(
                "export type {{ {} }} from \"{}\";\n",
                types.join(", "),
                path
            ));
            if !docs.is_empty() {
                output.push_str(&format!(
                    "export {{ {} }} from \"{}\";\n",
                    docs.join(", "),
                    path
                ));
            }
        }

        if let Some(import_path) = schema_import {
            output.push_str(&format!("export * from \"{}\";\n", import_path));
        }
    }

    output
}
