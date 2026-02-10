use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use apollo_compiler::ast::{OperationType, Type};
use apollo_compiler::executable::{self, Selection, SelectionSet};
use apollo_compiler::schema::ExtendedType;
use apollo_compiler::{Node, Schema};
use colored::*;
use dashmap::DashMap;
use graphox_core::apollo_ast::{
    get_fragment_fragment_dependencies, serialize_fragment_definition,
    serialize_operation_definition,
};
use graphox_core::config::FragmentMaskingConfig;
use graphox_core::document::DocumentState;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum FragmentMasking {
    Disabled,
    Enabled { unmask_function_name: String },
}

impl FragmentMasking {
    pub fn from_config(config: &Option<FragmentMaskingConfig>) -> Self {
        match config {
            None => FragmentMasking::Disabled,
            Some(c) => match &c.mode {
                graphox_core::config::FragmentMasking::Disabled => FragmentMasking::Disabled,
                graphox_core::config::FragmentMasking::Enabled {
                    unmask_function_name,
                } => FragmentMasking::Enabled {
                    unmask_function_name: unmask_function_name
                        .clone()
                        .unwrap_or_else(|| "getFragmentData".to_string()),
                },
            },
        }
    }

    pub fn is_enabled(&self) -> bool {
        matches!(self, FragmentMasking::Enabled { .. })
    }

    pub fn unmask_function_name(&self) -> &str {
        match self {
            FragmentMasking::Disabled => "getFragmentData",
            FragmentMasking::Enabled {
                unmask_function_name,
            } => unmask_function_name.as_str(),
        }
    }
}

pub struct CodegenContext<'a> {
    pub schema: &'a apollo_compiler::validation::Valid<Schema>,
    pub fragment_to_path: &'a HashMap<Arc<str>, Arc<str>>,
    pub fragment_to_import: &'a HashMap<Arc<str>, Arc<str>>,
    pub fragment_to_type_only: &'a HashMap<Arc<str>, bool>,
    pub all_fragments: &'a HashMap<String, Node<executable::Fragment>>,
    pub current_file_path: &'a Path,
    pub scalars: &'a Option<HashMap<String, String>>,
    pub schema_import: &'a Option<String>,
    pub generate_ast_for_fragments: bool,
    /// Cached fragment dependencies from workspace scan
    pub fragment_dependencies: &'a HashMap<Arc<str>, Vec<Arc<str>>>,
    /// Shared cache for type conversions across all files in a project (thread-safe)
    type_cache: &'a TypeCache,
    pub document_suffix: &'a str,
    pub variables_suffix: &'a str,
    pub fragment_suffix: &'a str,
    pub fragment_masking: FragmentMasking,
    pub masking_import_path: String,
}

/// Thread-safe cache for GraphQL type to TypeScript type conversions
/// Shared across all files in a project since they use the same schema
pub struct TypeCache {
    cache: DashMap<String, String>,
    // Optional metrics for benchmarking
    hits: AtomicUsize,
    misses: AtomicUsize,
}

impl Default for TypeCache {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeCache {
    pub fn new() -> Self {
        Self {
            cache: DashMap::new(),
            hits: AtomicUsize::new(0),
            misses: AtomicUsize::new(0),
        }
    }

    pub fn get_or_insert(&self, key: &str, compute: impl FnOnce() -> String) -> String {
        if let Some(cached) = self.cache.get(key) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return cached.clone();
        }

        self.misses.fetch_add(1, Ordering::Relaxed);
        let result = compute();
        self.cache.insert(key.to_string(), result.clone());
        result
    }

    pub fn stats(&self) -> (usize, usize) {
        (
            self.hits.load(Ordering::Relaxed),
            self.misses.load(Ordering::Relaxed),
        )
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }
}

impl<'a> CodegenContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        schema: &'a apollo_compiler::validation::Valid<Schema>,
        fragment_to_path: &'a HashMap<Arc<str>, Arc<str>>,
        fragment_to_import: &'a HashMap<Arc<str>, Arc<str>>,
        fragment_to_type_only: &'a HashMap<Arc<str>, bool>,
        all_fragments: &'a HashMap<String, Node<executable::Fragment>>,
        current_file_path: &'a Path,
        scalars: &'a Option<HashMap<String, String>>,
        schema_import: &'a Option<String>,
        generate_ast_for_fragments: bool,
        fragment_dependencies: &'a HashMap<Arc<str>, Vec<Arc<str>>>,
        type_cache: &'a TypeCache,
        document_suffix: &'a str,
        variables_suffix: &'a str,
        fragment_suffix: &'a str,
        fragment_masking: FragmentMasking,
        masking_import_path: String,
    ) -> Self {
        Self {
            schema,
            fragment_to_path,
            fragment_to_import,
            fragment_to_type_only,
            all_fragments,
            current_file_path,
            scalars,
            schema_import,
            generate_ast_for_fragments,
            fragment_dependencies,
            type_cache,
            document_suffix,
            variables_suffix,
            fragment_suffix,
            fragment_masking,
            masking_import_path,
        }
    }

    /// Get cached type conversion or compute and cache it
    fn get_cached_type(&self, type_name: &str, compute: impl FnOnce() -> String) -> String {
        self.type_cache.get_or_insert(type_name, compute)
    }
}

pub struct OperationGenerated {
    pub name: String,
    pub source_text: String,
    pub operation_type_name: String, // e.g. GetMeQuery
    pub variables_type_name: String, // e.g. GetMeQueryVariables
    pub codegen_path: PathBuf,       // Path to the .codegen.ts file
}

#[derive(Debug, Default)]
pub struct CodegenProfile {
    pub parse_time: std::time::Duration,
    pub selection_set_time: std::time::Duration,
    pub ast_serialization_time: std::time::Duration,
    pub import_generation_time: std::time::Duration,
}

pub fn generate_typescript(
    doc: &DocumentState,
    ctx: &CodegenContext,
) -> Result<(String, Vec<OperationGenerated>), String> {
    generate_typescript_with_profile(doc, ctx).map(|(s, ops, _)| (s, ops))
}

pub fn generate_typescript_with_profile(
    doc: &DocumentState,
    ctx: &CodegenContext,
) -> Result<(String, Vec<OperationGenerated>, CodegenProfile), String> {
    use std::time::Instant;
    let mut profile = CodegenProfile::default();

    // Pre-allocate output with estimated capacity
    let mut output = String::with_capacity(4096);
    output.push_str("/* tslint:disable */\n/* eslint-disable */\n// This file was automatically generated and should not be edited.\n\n");

    let mut used_fragments = HashMap::default();
    let mut generated_operations = Vec::new();

    // Pre-allocate bodies string
    let mut bodies = String::with_capacity(2048);
    let mut has_operations = false;
    let mut used_schema_types = HashSet::default();

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

            let sel_start = Instant::now();
            let ts_type = generate_selection_set(
                &op.selection_set,
                root_type,
                ctx,
                0,
                &mut used_fragments,
                &mut used_schema_types,
            );
            profile.selection_set_time += sel_start.elapsed();

            // Avoid format! macro overhead
            bodies.push_str("export interface ");
            bodies.push_str(name);
            bodies.push_str(suffix);
            bodies.push(' ');
            bodies.push_str(&ts_type);
            bodies.push_str("\n\n");

            let vars_type = if !op.variables.is_empty() {
                let v_name = format!("{}{}{}", name, suffix, ctx.variables_suffix);
                bodies.push_str("export interface ");
                bodies.push_str(&v_name);
                bodies.push_str(" {\n");
                for var in &op.variables {
                    let ts_type_str = gql_type_to_ts(
                        &var.ty,
                        ctx.schema,
                        ctx.scalars,
                        ctx,
                        &mut used_schema_types,
                    );
                    let optional = if var.ty.is_non_null() { "" } else { "?" };
                    bodies.push_str("  ");
                    bodies.push_str(&var.name);
                    bodies.push_str(optional);
                    bodies.push_str(": ");
                    bodies.push_str(&ts_type_str);
                    bodies.push_str(";\n");
                }
                bodies.push_str("}\n\n");
                v_name
            } else {
                "{ [key: string]: never; }".to_string()
            };

            let ast_start = Instant::now();
            let ast_content = if ctx.generate_ast_for_fragments {
                let op_def = serialize_operation_definition(op);

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
                graphox_core::apollo_ast::serialize_operation(op, ctx.all_fragments).to_string()
            };
            profile.ast_serialization_time += ast_start.elapsed();

            let doc_name = format!("{}{}", name, suffix);
            bodies.push_str("export const ");
            bodies.push_str(&doc_name);
            bodies.push_str(ctx.document_suffix);
            bodies.push_str(" = ");
            bodies.push_str(&ast_content);
            bodies.push_str(" as unknown as DocumentNode<");
            bodies.push_str(name);
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
            let ts_type = generate_selection_set(
                &frag.selection_set,
                type_def,
                ctx,
                0,
                &mut used_fragments,
                &mut used_schema_types,
            );
            profile.selection_set_time += sel_start.elapsed();

            bodies.push_str("export interface ");
            bodies.push_str(&frag.name);
            bodies.push_str(ctx.fragment_suffix);
            bodies.push(' ');
            bodies.push_str(&ts_type);

            if ctx.fragment_masking.is_enabled() {
                bodies.push_str("\n\n");
                bodies.push_str("export declare const ");
                bodies.push_str(&frag.name);
                bodies.push_str(ctx.fragment_suffix);
                bodies.push_str(": {\n");
                bodies.push_str("  __fragment: ");
                bodies.push_str(&ts_type);
                bodies.push_str(";\n");
                bodies.push_str("};\n");
            }
            bodies.push_str("\n\n");

            if ctx.generate_ast_for_fragments {
                let ast_start = Instant::now();
                let is_type_only = doc
                    .fragments()
                    .iter()
                    .find(|f| f.name.as_ref() == frag.name.as_str())
                    .map(|f| f.is_type_only)
                    .unwrap_or(false);

                if !is_type_only {
                    let frag_def = serialize_fragment_definition(frag);

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
                    bodies.push_str(&frag.name);
                    bodies.push_str(ctx.document_suffix);
                    bodies.push_str(" = { kind: 'Document', definitions: ");
                    bodies.push_str(&definitions);
                    bodies.push_str(" } as unknown as DocumentNode<any, any>;\n\n");
                }
                profile.ast_serialization_time += ast_start.elapsed();
            }
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
            // fragment_to_path contains absolute paths as strings.
            // doc.uri.path() is also absolute.
            if other_path.as_ref() != current_path {
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

    if let Some(schema_import_path) = ctx.schema_import
        && !used_schema_types.is_empty()
    {
        // Use BTreeSet to keep schema types sorted
        let types: std::collections::BTreeSet<_> = used_schema_types.into_iter().collect();

        // Pre-allocate string for import line
        let estimated_size = types.len() * 20 + schema_import_path.len() + 30;
        let mut line = String::with_capacity(estimated_size);
        line.push_str("import type { ");
        let mut first = true;
        for ty in types {
            if !first {
                line.push_str(", ");
            }
            first = false;
            line.push_str(&ty);
        }
        line.push_str(" } from \"");
        line.push_str(schema_import_path);
        line.push_str("\";\n");
        import_section.push_str(&line);
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
            final_path_str
        };

        let suffixed_names: Vec<_> = names
            .iter()
            .map(|n| format!("{}{}", n, ctx.fragment_suffix))
            .collect();

        import_section.push_str("import type { ");
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

    if has_operations {
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

    output.push_str(&bodies);
    profile.import_generation_time = import_start.elapsed();

    if bodies.is_empty() {
        return Err("No executable operations or fragments found in this file".to_string());
    }

    Ok((output, generated_operations, profile))
}

pub fn generate_entrypoint_content(
    output_dir: &Path,
    operations: &[OperationGenerated],
    document_suffix: &str,
    variables_suffix: &str,
    fragment_masking: &FragmentMasking,
) -> String {
    // Pre-allocate with estimated capacity based on number of operations
    let estimated_size = operations.len() * 200 + 500; // ~200 chars per operation + overhead
    let mut output = String::with_capacity(estimated_size);
    output.push_str("/* tslint:disable */\n/* eslint-disable */\n// This file was automatically generated and should not be edited.\n\n");

    if fragment_masking.is_enabled() {
        output.push_str("import type { FragmentType } from \"./fragment-masking\";\n");
    }
    output.push_str("import type { TypedDocumentNode as DocumentNode } from \"@graphql-typed-document-node/core\";\n");

    let mut import_lines = Vec::with_capacity(operations.len());
    let mut overloads = String::with_capacity(operations.len() * 100);
    let mut map_entries = String::with_capacity(operations.len() * 80);

    for op in operations {
        let rel_codegen_path = pathdiff::diff_paths(&op.codegen_path, output_dir)
            .unwrap_or_else(|| op.codegen_path.clone());
        let mut path_str = graphox_core::utils::to_posix_path(&rel_codegen_path);
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
            "import {{ {}, {}{}, {}{} }} from \"{}\";",
            op.operation_type_name,
            op.operation_type_name,
            variables_suffix,
            op.operation_type_name,
            document_suffix,
            path_no_ext
        ));

        // Write overloads using format! for proper string escaping
        overloads.push_str(&format!(
            "export function graphql(source: {:?}): typeof {}{};\n",
            op.source_text, op.operation_type_name, document_suffix
        ));

        // Write map entries using format! for proper string escaping
        map_entries.push_str(&format!(
            "  {:?}: {}{},\n",
            op.source_text, op.operation_type_name, document_suffix
        ));
    }

    import_lines.sort();
    import_lines.dedup();
    for line in import_lines {
        output.push_str(&line);
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

pub fn emit_permission_data_content(
    schema: &apollo_compiler::validation::Valid<Schema>,
    scalars: &Option<HashMap<String, String>>,
    schema_import: &Option<String>,
) -> String {
    let mut output = String::with_capacity(2048);
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

        if let Some(fields) = fields
            && let Some(permissions_field) = fields.get("permissions")
        {
            let inner_name = permissions_field.ty.inner_named_type();
            let inner_type = schema.types.get(inner_name.as_str());
            if let Some(ExtendedType::Enum(_)) = inner_type {
                types_with_permissions.push((name, permissions_field));
            } else {
                eprintln!(
                    "{}: Type '{}' has a 'permissions' field, but its type '{}' is not an enum. Skipping permissions generation for this type.",
                    "Warning".yellow(),
                    name.blue(),
                    inner_name.blue()
                );
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
    let empty_deps = HashMap::default();
    let empty_path_map = HashMap::default();
    let empty_import_map = HashMap::default();
    let empty_type_only_map = HashMap::default();
    let dummy_cache = TypeCache::new();
    let dummy_ctx = CodegenContext::new(
        schema,
        &empty_path_map,
        &empty_import_map,
        &empty_type_only_map,
        &empty_fragments,
        Path::new(""),
        scalars,
        schema_import,
        false,
        &empty_deps,
        &dummy_cache,
        "Document",
        "Variables",
        "",
        FragmentMasking::Disabled,
        "./fragment-masking".to_string(),
    );
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
    let categorized = categorize_selections(selection_set, used_fragments);

    if categorized.inline_fragments.is_empty() {
        generate_object_or_intersection(
            &categorized,
            parent_type,
            ctx,
            indent,
            used_fragments,
            used_schema_types,
        )
    } else {
        generate_union_type(
            &categorized.inline_fragments,
            &categorized.fragment_spreads,
            parent_type,
            ctx,
            indent,
            used_fragments,
            used_schema_types,
        )
    }
}

/// Categorized results from a selection set
struct CategorizedSelections<'a> {
    fields: Vec<&'a Node<executable::Field>>,
    inline_fragments: Vec<&'a Node<executable::InlineFragment>>,
    fragment_spreads: Vec<&'a Node<executable::FragmentSpread>>,
    has_explicit_typename: bool,
}

/// Categorize selections into fields, inline fragments, and fragment spreads
fn categorize_selections<'a>(
    selection_set: &'a SelectionSet,
    used_fragments: &mut HashMap<String, String>,
) -> CategorizedSelections<'a> {
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

    CategorizedSelections {
        fields,
        inline_fragments,
        fragment_spreads,
        has_explicit_typename,
    }
}

/// Generate TypeScript type for object or intersection types (no inline fragments)
fn generate_object_or_intersection(
    categorized: &CategorizedSelections,
    parent_type: &ExtendedType,
    ctx: &CodegenContext,
    indent: usize,
    used_fragments: &mut HashMap<String, String>,
    used_schema_types: &mut HashSet<String>,
) -> String {
    let local_fields_list = generate_field_list(
        &categorized.fields,
        parent_type,
        ctx,
        indent,
        categorized.has_explicit_typename,
        used_fragments,
        used_schema_types,
    );

    if categorized.fragment_spreads.is_empty() {
        format_multiline_object(&local_fields_list, indent)
    } else {
        format_intersection(&local_fields_list, &categorized.fragment_spreads, ctx)
    }
}

/// Generate list of TypeScript field definitions
fn generate_field_list(
    fields: &[&Node<executable::Field>],
    parent_type: &ExtendedType,
    ctx: &CodegenContext,
    indent: usize,
    has_explicit_typename: bool,
    used_fragments: &mut HashMap<String, String>,
    used_schema_types: &mut HashSet<String>,
) -> Vec<String> {
    let mut local_fields_list = Vec::with_capacity(fields.len() + 1);

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

    local_fields_list
}

/// Format fields as a multi-line TypeScript object
fn format_multiline_object(fields: &[String], indent: usize) -> String {
    let pad = "  ".repeat(indent);
    let inner_pad = "  ".repeat(indent + 1);

    let estimated_size = fields.len() * 40 + 20;
    let mut result = String::with_capacity(estimated_size);

    for f in fields {
        result.push('\n');
        result.push_str(&inner_pad);
        result.push_str(f);
        result.push(';');
    }
    result.push('\n');
    result.push_str(&pad);

    let mut output = String::with_capacity(result.len() + 2);
    output.push('{');
    output.push_str(&result);
    output.push('}');
    output
}

/// Format as TypeScript intersection type (object & fragments) or FragmentType wrapper
fn format_intersection(
    fields: &[String],
    fragment_spreads: &[&Node<executable::FragmentSpread>],
    ctx: &CodegenContext,
) -> String {
    let base_obj = format!("{{ {} }}", fields.join(", "));
    let mut spreads: Vec<_> = fragment_spreads
        .iter()
        .map(|s| {
            format!(
                "FragmentType<typeof {}{}>",
                s.fragment_name.as_str(),
                ctx.fragment_suffix
            )
        })
        .collect();
    spreads.sort();

    if ctx.fragment_masking.is_enabled() {
        if spreads.is_empty() {
            base_obj
        } else if fields.len() == 1 && fields[0].starts_with("__typename:") {
            if spreads.len() == 1 {
                spreads[0].clone()
            } else {
                format!("({})", spreads.join(" & "))
            }
        } else {
            format!("({} & {})", base_obj, spreads.join(" & "))
        }
    } else {
        let mut plain_spreads: Vec<_> = fragment_spreads
            .iter()
            .map(|s| format!("{}{}", s.fragment_name.as_str(), ctx.fragment_suffix))
            .collect();
        plain_spreads.sort();
        format!("({} & {})", base_obj, plain_spreads.join(" & "))
    }
}

/// Generate TypeScript union type for inline fragments
fn generate_union_type(
    inline_fragments: &[&Node<executable::InlineFragment>],
    fragment_spreads: &[&Node<executable::FragmentSpread>],
    parent_type: &ExtendedType,
    ctx: &CodegenContext,
    indent: usize,
    used_fragments: &mut HashMap<String, String>,
    used_schema_types: &mut HashSet<String>,
) -> String {
    let pad = "  ".repeat(indent);
    let mut branches = Vec::with_capacity(inline_fragments.len() + fragment_spreads.len() + 1);

    // Base branch
    branches.push(format!("{{ __typename: \"{}\" }}", parent_type.name()));

    // Add inline fragment branches
    for inline in inline_fragments {
        let branch = generate_inline_fragment_branch(
            inline,
            parent_type,
            ctx,
            indent,
            used_fragments,
            used_schema_types,
        );
        branches.push(branch);
    }

    // Add fragment spread branches
    for spread in fragment_spreads {
        branches.push(spread.fragment_name.to_string());
        used_fragments.insert(spread.fragment_name.to_string(), String::new());
    }

    format_union_branches(&branches, &pad)
}

/// Generate a single inline fragment branch
fn generate_inline_fragment_branch(
    inline: &Node<executable::InlineFragment>,
    parent_type: &ExtendedType,
    ctx: &CodegenContext,
    indent: usize,
    used_fragments: &mut HashMap<String, String>,
    used_schema_types: &mut HashSet<String>,
) -> String {
    let pad = "  ".repeat(indent);
    let type_name = inline
        .type_condition
        .as_ref()
        .map(|n| n.as_str())
        .unwrap_or_else(|| parent_type.name());
    let target_type = ctx.schema.types.get(type_name).unwrap_or(parent_type);

    let mut branch_fields = String::with_capacity(256);
    branch_fields.push('\n');
    branch_fields.push_str(&pad);
    branch_fields.push_str("    __typename: \"");
    branch_fields.push_str(type_name);
    branch_fields.push_str("\";");

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
                branch_fields.push('\n');
                branch_fields.push_str(&pad);
                branch_fields.push_str("    ");
                branch_fields.push_str(name);
                branch_fields.push_str(": ");
                branch_fields.push_str(&ts_type);
                branch_fields.push(';');
            }
        }
    }

    let mut branch = String::with_capacity(branch_fields.len() + pad.len() + 10);
    branch.push('{');
    branch.push_str(&branch_fields);
    branch.push('\n');
    branch.push_str(&pad);
    branch.push_str("  }");
    branch
}

/// Format union type branches with proper separators
fn format_union_branches(branches: &[String], pad: &str) -> String {
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
            // Use cache for expensive enum value lookups
            ctx.get_cached_type(other, || {
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
                                // This is expensive - building the union of all enum values
                                // Cache it so we only do it once per enum type
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
            })
        }
    };

    generate_ts_type(ty, &base)
}

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
    let dummy_ctx = CodegenContext::new(
        schema,
        &empty_path_map,
        &empty_import_map,
        &empty_type_only_map,
        &empty_fragments,
        Path::new(""),
        scalars,
        &None,
        false,
        &empty_deps,
        &dummy_cache,
        "Document",
        "Variables",
        "",
        FragmentMasking::Disabled,
        "./fragment-masking".to_string(),
    );
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

/// Get operation dependencies using cached fragment dependencies when available
/// Falls back to tree traversal if cache is empty (shouldn't happen in normal flow)
fn get_operation_deps_cached(
    operation: &executable::Operation,
    ctx: &CodegenContext,
    doc: &DocumentState,
) -> HashSet<Arc<str>> {
    let mut all_deps = HashSet::default();

    // Collect direct fragment spreads from the operation (single pass)
    collect_direct_fragment_spreads(&operation.selection_set, &mut all_deps);

    // Pre-allocate for transitive deps to reduce reallocations
    let initial_size = all_deps.len();
    let mut transitive_deps: HashSet<Arc<str>> =
        HashSet::with_capacity_and_hasher(initial_size * 2, Default::default());

    // For each direct dependency, add its transitive dependencies from cache
    for frag_name in &all_deps {
        if let Some(cached_transitive) = ctx.fragment_dependencies.get(&frag_name[..]) {
            // Use cached transitive dependencies - avoid cloning
            transitive_deps.extend(cached_transitive.iter().cloned());
        } else {
            // Fallback: compute manually (only for fragments defined in current file)
            if let Some(local_frag) = doc
                .fragments()
                .iter()
                .find(|f| f.name.as_ref() == frag_name.as_ref())
            {
                // This fragment is local, compute its deps on the fly
                if let Some(parsed_frag) = ctx.all_fragments.get(local_frag.name.as_ref()) {
                    let frag_deps =
                        get_fragment_fragment_dependencies(parsed_frag, ctx.all_fragments);
                    transitive_deps.extend(frag_deps.into_iter().map(|s| s.into()));
                }
            }
        }
    }

    // Merge transitive deps into all_deps
    all_deps.extend(transitive_deps);
    all_deps
}

/// Get fragment dependencies using cache
fn get_fragment_deps_cached(fragment_name: &str, ctx: &CodegenContext) -> HashSet<Arc<str>> {
    if let Some(cached_deps) = ctx.fragment_dependencies.get(fragment_name) {
        // Use cached dependencies
        cached_deps.iter().cloned().collect()
    } else {
        // Fallback: shouldn't happen in normal flow
        HashSet::default()
    }
}

/// Collect direct fragment spreads from a selection set (non-recursive)
fn collect_direct_fragment_spreads(
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

/// Generate fragment-masking.ts utility file content
pub fn generate_fragment_masking_file(unmask_function_name: &str) -> String {
    format!(
        r#"/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export type FragmentType<TFragment> = TFragment extends {{ __fragment: infer T }}
  ? T
  : never;

export function {}<TFragment, TData>(
  _fragment: TFragment,
  data: TData
): FragmentType<TFragment> {{
  return data as FragmentType<TFragment>;
}}
"#,
        unmask_function_name
    )
}
