use apollo_compiler::ast::OperationType;
use graphox_core::config::CodegenConfig;
use std::collections::BTreeMap;
use std::path::Path;

use crate::context::{FragmentGenerated, OperationGenerated};

/// Drops the copies of a definition that reach one output directory more than
/// once, as when projects that share the directory both include its file, and
/// puts what is left in a stable order.
///
/// A copy is the same name and the same source text. The name is part of it
/// because every definition in a document carries the whole document as its
/// source: two fragments written in one file share a source text and are
/// still two fragments.
pub fn dedup_definitions(
    operations: &mut Vec<OperationGenerated>,
    fragments: &mut Vec<FragmentGenerated>,
) {
    operations.sort_by(|a, b| {
        a.operation_type_name
            .cmp(&b.operation_type_name)
            .then_with(|| a.source_text.cmp(&b.source_text))
    });
    operations.dedup_by(|a, b| {
        a.operation_type_name == b.operation_type_name && a.source_text == b.source_text
    });

    fragments.sort_by(|a, b| {
        a.fragment_type_name
            .cmp(&b.fragment_type_name)
            .then_with(|| a.source_text.cmp(&b.source_text))
    });
    fragments.dedup_by(|a, b| {
        a.fragment_type_name == b.fragment_type_name && a.source_text == b.source_text
    });
}

/// The definition `graphql(source)` returns for each source text. One block
/// can define several definitions, which all share its source text, while
/// `graphql()` returns one document for it: the block's first operation by
/// type name, or, in a block of fragments alone, its first fragment by name.
pub struct GraphqlOverloads<'a> {
    /// Keyed and ordered by source text.
    pub operations: BTreeMap<&'a str, &'a OperationGenerated>,
    /// In name order, each for a source text no operation has.
    pub fragments: Vec<&'a FragmentGenerated>,
}

impl<'a> GraphqlOverloads<'a> {
    pub fn new(operations: &'a [OperationGenerated], fragments: &'a [FragmentGenerated]) -> Self {
        let mut by_source: BTreeMap<&'a str, &'a OperationGenerated> = BTreeMap::new();
        for op in operations {
            by_source.entry(op.source_text.as_str()).or_insert(op);
        }

        let mut by_name: BTreeMap<&'a str, &'a FragmentGenerated> = BTreeMap::new();
        for frag in fragments {
            by_name.entry(frag.name.as_str()).or_insert(frag);
        }
        let mut taken: ahash::AHashSet<&'a str> = by_source.keys().copied().collect();
        let fragments = by_name
            .into_values()
            .filter(|frag| taken.insert(frag.source_text.as_str()))
            .collect();

        Self {
            operations: by_source,
            fragments,
        }
    }

    /// The document `graphql(source)` returns, by its export name.
    pub fn document_for(&self, source: &str) -> Option<&'a str> {
        self.operations
            .get(source)
            .map(|op| op.document_name.as_str())
            .or_else(|| {
                self.fragments
                    .iter()
                    .find(|frag| frag.source_text == source)
                    .map(|frag| frag.document_name.as_str())
            })
    }
}

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

    let graphql_overloads = GraphqlOverloads::new(operations, fragments);
    // Deduplicate operations by name to avoid duplicate imports
    let mut unique_ops_by_name = BTreeMap::new();

    for op in operations {
        unique_ops_by_name
            .entry(&op.operation_type_name)
            .or_insert(op);
    }

    for op in unique_ops_by_name.values() {
        let rel_codegen_path = get_relative_path(&op.codegen_path);
        let mut path_str = graphox_core::utils::to_posix_path(&rel_codegen_path);
        if !path_str.starts_with('.')
            && !path_str.starts_with('/')
            && !rel_codegen_path.is_absolute()
        {
            path_str = format!("./{}", path_str);
        }
        let path_no_ext = if path_str.ends_with(".ts") {
            &path_str[..path_str.len() - 3]
        } else {
            &path_str
        };
        let final_path = format!("{}{}", path_no_ext, ext);

        type_import_lines.push(format!(
            "import type {{ {} }} from \"{}\";",
            std::iter::once(op.operation_type_name.as_str())
                .chain(std::iter::once(op.variables_type_name.as_str()))
                .collect::<Vec<_>>()
                .join(", "),
            final_path
        ));

        runtime_import_lines.push(format!(
            "import {{ {} }} from \"{}\";",
            op.document_name, final_path
        ));
    }

    for op in graphql_overloads.operations.values() {
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

    for frag in unique_frags_by_name.values() {
        let rel_codegen_path = get_relative_path(&frag.codegen_path);
        let mut path_str = graphox_core::utils::to_posix_path(&rel_codegen_path);
        if !path_str.starts_with('.')
            && !path_str.starts_with('/')
            && !rel_codegen_path.is_absolute()
        {
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

    for frag in &graphql_overloads.fragments {
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

            if !graphql_tag_fallback {
                map_entries.push_str(&format!("  {:?}: {{}},\n", frag.source_text));
            }
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

    if graphql_tag_fallback {
        output.push_str("const fragmentSources: { [key: string]: string } = {\n");
        for frag in unique_frags_by_name.values() {
            output.push_str(&format!("  {:?}: {:?},\n", frag.name, frag.source_text));
        }
        output.push_str("};\n");
        output.push_str(
            "const FRAGMENT_SPREAD_PATTERN = /\\.\\.\\.\\s*(?!on\\b)([A-Za-z_][A-Za-z0-9_]*)/g;\n",
        );
        output.push_str(
            "function sourceIncludesFragment(source: string, fragmentName: string): boolean {\n  return new RegExp(`fragment\\\\s+${fragmentName}\\\\s+on\\\\b`).test(source);\n}\n",
        );
        output.push_str(
            "function withFragmentDefinitions(source: string): string {\n  const pending = [source];\n  const appended: string[] = [];\n  const seen = new Set<string>();\n  while (pending.length > 0) {\n    const current = pending.pop() || \"\";\n    FRAGMENT_SPREAD_PATTERN.lastIndex = 0;\n    let match: RegExpExecArray | null;\n    while ((match = FRAGMENT_SPREAD_PATTERN.exec(current)) !== null) {\n      const fragmentName = match[1];\n      if (seen.has(fragmentName) || sourceIncludesFragment(source, fragmentName)) {\n        continue;\n      }\n      seen.add(fragmentName);\n      const fragmentSource = fragmentSources[fragmentName];\n      if (!fragmentSource) {\n        continue;\n      }\n      appended.push(fragmentSource);\n      pending.push(fragmentSource);\n    }\n  }\n  return appended.length === 0 ? source : `${source}\\n${appended.join(\"\\n\")}`;\n}\n\n",
        );
    }

    output.push_str("const documents: { [key: string]: any } = {\n");
    output.push_str(&map_entries);
    output.push_str("};\n\n");

    output.push_str(&overloads);
    output.push_str("export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;\n");
    if graphql_tag_fallback {
        output.push_str(
            "export function graphql(source: string): any {\n  return documents[source] || (documents[source] = gqlTag(withFragmentDefinitions(source)));\n}\n\n",
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
            if !path_str.starts_with('.')
                && !path_str.starts_with('/')
                && !rel_codegen_path.is_absolute()
            {
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
            let mut values = Vec::new();
            for op in ops {
                types.push(op.operation_type_name.clone());
                types.push(op.variables_type_name.clone());
                if codegen_config.react_apollo_hooks() {
                    match op.operation_type {
                        OperationType::Query => {
                            let base_name = op
                                .operation_type_name
                                .strip_suffix(codegen_config.query_suffix())
                                .unwrap_or(&op.operation_type_name);
                            types.push(format!("{}QueryHookResult", base_name));
                            types.push(format!("{}LazyQueryHookResult", base_name));
                            types.push(format!("{}QueryResult", base_name));
                        }
                        OperationType::Mutation => {
                            let base_name = op
                                .operation_type_name
                                .strip_suffix(codegen_config.mutation_suffix())
                                .unwrap_or(&op.operation_type_name);
                            types.push(format!("{}MutationHookResult", base_name));
                            types.push(format!("{}MutationResult", base_name));
                        }
                        OperationType::Subscription => {}
                    }
                    values.extend(op.hook_names.iter().cloned());
                }
                docs.push(op.document_name.clone());
            }
            types.sort();
            types.dedup();
            docs.sort();
            docs.dedup();
            values.sort();
            values.dedup();
            output.push_str(&format!(
                "export type {{ {} }} from \"{}\";\n",
                types.join(", "),
                path
            ));
            if !docs.is_empty() || !values.is_empty() {
                let exports = docs
                    .iter()
                    .chain(values.iter())
                    .cloned()
                    .collect::<Vec<_>>();
                output.push_str(&format!(
                    "export {{ {} }} from \"{}\";\n",
                    exports.join(", "),
                    path
                ));
            }
        }

        let mut path_to_frags: BTreeMap<String, Vec<&FragmentGenerated>> = BTreeMap::new();
        for frag in unique_frags_by_name.values() {
            let rel_codegen_path = get_relative_path(&frag.codegen_path);
            let mut path_str = graphox_core::utils::to_posix_path(&rel_codegen_path);
            if !path_str.starts_with('.')
                && !path_str.starts_with('/')
                && !rel_codegen_path.is_absolute()
            {
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
