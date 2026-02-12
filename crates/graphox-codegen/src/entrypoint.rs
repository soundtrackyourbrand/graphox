use graphox_core::config::EmitExtensions;
use std::collections::BTreeMap;
use std::path::Path;

use crate::context::{FragmentGenerated, FragmentMasking, OperationGenerated};

#[allow(clippy::too_many_arguments)]
pub fn generate_entrypoint_content(
    output_dir: &Path,
    operations: &[OperationGenerated],
    fragments: &[FragmentGenerated],
    document_suffix: &str,
    variables_suffix: &str,
    fragment_masking: &FragmentMasking,
    emit_extensions: EmitExtensions,
    generate_ast_for_fragments: bool,
) -> String {
    let op_count = operations.len();
    let frag_count = fragments.len();
    let estimated_size = (op_count + frag_count) * 200 + 500;
    let mut output = String::with_capacity(estimated_size);
    output.push_str("/* tslint:disable */\n/* eslint-disable */\n// This file was automatically generated and should not be edited.\n\n");

    let ext = emit_extensions.as_str();
    if fragment_masking.is_enabled() {
        output.push_str(&format!(
            "import type {{ FragmentType }} from \"./fragment-masking{}\";\n",
            ext
        ));
    }
    output.push_str("import type { TypedDocumentNode as DocumentNode } from \"@graphql-typed-document-node/core\";\n");

    output.push_str("type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;\n");

    if fragment_masking.is_enabled() {
        output.push_str("export type Incremental<T> = T | { [P in keyof T]?: P extends ' $fragmentName' | '__typename' ? T[P] : never };\n");
    }

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
        let rel_codegen_path = pathdiff::diff_paths(&op.codegen_path, output_dir)
            .unwrap_or_else(|| op.codegen_path.clone());
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
            "import type {{ {}, {}{} }} from \"{}\";",
            op.operation_type_name, op.operation_type_name, variables_suffix, final_path
        ));

        runtime_import_lines.push(format!(
            "import {{ {}{} }} from \"{}\";",
            op.operation_type_name, document_suffix, final_path
        ));
    }

    for op in unique_ops_by_source.values() {
        overloads.push_str(&format!(
            "export function graphql(source: {:?}): typeof {}{};\n",
            op.source_text, op.operation_type_name, document_suffix
        ));

        map_entries.push_str(&format!(
            "  {:?}: {}{},\n",
            op.source_text, op.operation_type_name, document_suffix
        ));
    }

    let mut unique_frags_by_source = BTreeMap::new();
    let mut unique_frags_by_name = BTreeMap::new();

    for frag in fragments {
        unique_frags_by_source
            .entry(&frag.source_text)
            .or_insert(frag);
        unique_frags_by_name.entry(&frag.name).or_insert(frag);
    }

    // Track source texts already added (from operations) to avoid duplicate keys
    let mut added_source_texts: Vec<&String> = unique_ops_by_source.keys().cloned().collect();

    for frag in unique_frags_by_name.values() {
        let rel_codegen_path = pathdiff::diff_paths(&frag.codegen_path, output_dir)
            .unwrap_or_else(|| frag.codegen_path.clone());
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
                frag.name, final_path
            ));
        }
    }

    for frag in unique_frags_by_source.values() {
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
                frag.source_text, frag.name
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
    output.push_str(
        "export function graphql(source: string): any {\n  return documents[source] || {};\n}\n\n",
    );
    output.push_str("export const gql = graphql;\n");

    output
}
