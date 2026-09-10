//! The preparation generating TypeScript needs, before any of it is generated.
//!
//! `codegen` writes the output; `analyze codegen` measures it. Both have to
//! resolve the same things first — the workspace scan, each distinct schema
//! validated once, where a generated file imports its schema types from, and
//! where each file's output would go — so that lives here rather than in either
//! of them.

use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use graphox_core::config::{Config, EmitExtensions, GlobPattern, ProjectConfig, SchemaTypeConfig};
use graphox_core::engine::{Engine, WorkspaceMetadata};
use graphox_core::schema;
use graphox_core::utils;
use rayon::prelude::*;
use std::path::{Path, PathBuf};

use super::ValidSchema;

pub(crate) struct CodegenSetup<'a> {
    pub workspace: WorkspaceMetadata,
    pub schemas: HashMap<String, Result<ValidSchema, String>>,
    /// Per schema key, the `type name -> import path` map a generated file
    /// against that schema uses. A std map because that is what rayon can
    /// collect a parallel iterator into.
    type_imports: std::collections::HashMap<String, HashMap<String, String>>,
    /// Per schema key, the `schema_types` entries whose files that source
    /// covers, most specific first.
    matches: HashMap<String, Vec<&'a SchemaTypeConfig>>,
}

impl<'a> CodegenSetup<'a> {
    pub fn resolve(config: &'a Config) -> Self {
        // Cheap, synchronous prep: the distinct schema sources, and per source the
        // `schema_types` whose files that source covers (used for type-import mapping and
        // the schema-import fallback in the project loop).
        let schema_types = config.schema_types();
        let mut unique_sources = HashSet::new();
        for project in config.projects() {
            unique_sources.insert(project.schema().as_key());
        }
        let matches: HashMap<String, Vec<_>> = unique_sources
            .iter()
            .map(|key| {
                let schema_files: HashSet<_> = key.split(',').map(String::from).collect();
                let mut matches: Vec<_> = schema_types
                    .iter()
                    .filter(|st| {
                        let st_files = st.schema().files();
                        st_files.iter().all(|f| schema_files.contains(f))
                    })
                    .collect();
                matches.sort_by_key(|st| std::cmp::Reverse(st.schema().files().len()));
                (key.clone(), matches)
            })
            .collect();

        // The workspace scan, project-schema validation, and schema_types type-import
        // precompute are mutually independent and each non-trivial. Run them concurrently,
        // and — crucially — validate each DISTINCT schema once rather than per project
        // (many projects share the same large schema).
        let (workspace, (schemas, type_imports)) = rayon::join(
            || {
                Engine::scan_workspace(
                    config,
                    tower_lsp_server::ls_types::PositionEncodingKind::UTF8,
                    None,
                )
            },
            || {
                rayon::join(
                    || super::build_validated_schemas(config),
                    || -> std::collections::HashMap<String, HashMap<String, String>> {
                        unique_sources
                            .par_iter()
                            .map(|key| {
                                let mut project_type_imports = HashMap::default();
                                if let Some(matches) = matches.get(key) {
                                    for st in matches.iter().rev() {
                                        if let Some(import_path) = st.import()
                                            && let Ok(st_schema) = schema::load_schema_with_cache(
                                                config.base_dir(),
                                                st.schema(),
                                                config.enable_schema_cache(),
                                            )
                                        {
                                            for type_name in st_schema.types.keys() {
                                                project_type_imports.insert(
                                                    type_name.to_string(),
                                                    import_path.to_string(),
                                                );
                                            }
                                        }
                                    }
                                }
                                (key.clone(), project_type_imports)
                            })
                            .collect()
                    },
                )
            },
        );

        Self {
            workspace,
            schemas,
            type_imports,
            matches,
        }
    }

    pub fn type_imports_for(&self, schema_key: &str) -> &HashMap<String, String> {
        static EMPTY: std::sync::LazyLock<HashMap<String, String>> =
            std::sync::LazyLock::new(HashMap::default);
        self.type_imports.get(schema_key).unwrap_or(&EMPTY)
    }

    /// Where a project's generated files import their schema types from: its own
    /// `import`, else the `schema_types` entry covering its schema — rewritten
    /// to a relative path when that entry emits into the project's own output
    /// directory.
    pub fn schema_import(&self, config: &Config, project: &ProjectConfig) -> Option<String> {
        if let Some(import) = project.import() {
            return Some(import.to_string());
        }

        let st = self.matches.get(&project.schema().as_key())?.first()?;
        let project_abs_out_dir = project
            .output_dir()
            .map(|dir| config.base_dir().join(Path::new(dir)));
        let mut import_path = st.import().map(String::from);

        if let Some(path) = &import_path
            && (path == "." || path == "./")
            && let Some(abs_out_dir) = &project_abs_out_dir
        {
            let abs_st_output = config.base_dir().join(st.output());
            if abs_st_output.parent() == Some(abs_out_dir.as_path()) {
                let rel = pathdiff::diff_paths(&abs_st_output, abs_out_dir)
                    .unwrap_or_else(|| PathBuf::from(abs_st_output.file_name().unwrap()));
                let mut s = utils::to_posix_path(&rel);
                if s.ends_with(".ts") {
                    s.truncate(s.len() - 3);
                }
                if !s.starts_with('.') {
                    s = format!("./{}", s);
                }
                import_path = Some(s);
            }
        }

        import_path
    }
}

/// Where one source file's generated output goes, and what it imports from.
pub(crate) struct FilePaths {
    /// The matched `include` root, stripped from the output path so a project's
    /// tree is mirrored under its `output_dir` rather than nested under the
    /// whole source path.
    pub include_prefix: PathBuf,
    pub out_path: PathBuf,
    pub masking_import_path: String,
}

pub(crate) fn file_paths(
    base_dir: &Path,
    include: &GlobPattern,
    output_dir: Option<&Path>,
    emit_extensions: EmitExtensions,
    path: &Path,
) -> FilePaths {
    let include_prefix = include
        .patterns()
        .iter()
        .map(|p| utils::get_glob_root(p))
        .find(|root| {
            let abs_root = base_dir.join(root);
            let abs_root = std::fs::canonicalize(&abs_root).unwrap_or(abs_root);
            utils::path_starts_with(path, &abs_root)
        })
        .unwrap_or_default();

    let out_path_raw = utils::get_output_path(path, base_dir, output_dir, Some(&include_prefix));
    let out_path = if out_path_raw.is_absolute() {
        out_path_raw
    } else {
        base_dir.join(out_path_raw)
    };

    let masking_import_path = if let Some(out_dir) = output_dir {
        let abs_out_dir = base_dir.join(out_dir);
        let abs_file_out_dir = out_path.parent().unwrap();

        let rel_to_masking = pathdiff::diff_paths(&abs_out_dir, abs_file_out_dir)
            .unwrap_or_else(|| PathBuf::from("."));

        let full_masking_path = rel_to_masking.join("fragment-masking");
        let mut path_str = utils::to_posix_path(&full_masking_path);
        if !path_str.starts_with('.')
            && !path_str.starts_with('/')
            && !full_masking_path.is_absolute()
        {
            path_str.insert_str(0, "./");
        }
        path_str.push_str(emit_extensions.as_str());
        path_str
    } else {
        let mut path_str = "./fragment-masking".to_string();
        path_str.push_str(emit_extensions.as_str());
        path_str
    };

    FilePaths {
        include_prefix,
        out_path,
        masking_import_path,
    }
}
