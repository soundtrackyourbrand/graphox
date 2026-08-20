use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use swc_core::common::{DUMMY_SP, SyntaxContext};
use swc_core::ecma::ast::*;
use swc_core::ecma::visit::{Visit, VisitMut, VisitMutWith, VisitWith};
use swc_core::plugin::{metadata::TransformPluginProgramMetadata, plugin_transform};

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub enum EmitExtensions {
    #[default]
    None,
    Ts,
    Dts,
    Js,
}

impl EmitExtensions {
    pub fn as_str(&self) -> &'static str {
        match self {
            EmitExtensions::None => "",
            EmitExtensions::Ts => ".ts",
            EmitExtensions::Dts => ".d.ts",
            EmitExtensions::Js => ".js",
        }
    }
}

/// One codegen output directory, i.e. one graphox project.
///
/// A workspace resolves GraphQL against several of these, and a module belongs
/// to exactly one. Registering them all in a single plugin instance lets one
/// pass rewrite imports that cross project boundaries, and avoids paying an AST
/// round-trip per project per module.
#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct OutputConfig {
    /// Absolute path to the output directory where codegen files are located
    #[serde(rename = "outputDir")]
    pub output_dir: String,
    /// Bare specifier other projects use to import this output, e.g.
    /// `@example/catalog/graphql`. Required for a document in this output to
    /// be importable from another project: the rewritten import becomes
    /// `<importAlias>/<codegen file>`, which needs a matching subpath export.
    /// Configured explicitly rather than inferred from `exports`.
    #[serde(rename = "importAlias")]
    pub import_alias: Option<String>,
    #[serde(rename = "manifestPath")]
    pub manifest_path: Option<String>,
    #[serde(rename = "manifestData")]
    pub manifest_data: Option<Vec<ManifestEntry>>,
    #[serde(rename = "graphqlImportPaths")]
    pub graphql_import_paths: Option<Vec<String>>,
    /// Root of the package owning this output. A module inside it imports these
    /// documents by relative path; anything outside has to go through
    /// `importAlias`, because a relative path would reach past the package's
    /// subpath exports. Absent means "always relative", which is the behaviour
    /// of the single-output form.
    #[serde(rename = "packageRoot")]
    pub package_root: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, Default)]
pub struct Config {
    #[serde(rename = "manifestPath")]
    pub manifest_path: Option<String>,
    #[serde(rename = "manifestData")]
    pub manifest_data: Option<Vec<ManifestEntry>>,
    /// Absolute path to the output directory where codegen files are located.
    /// Deprecated single-output form, equivalent to a one-element `outputs`.
    #[serde(rename = "outputDir", default)]
    pub output_dir: String,
    #[serde(rename = "graphqlImportPaths")]
    pub graphql_import_paths: Option<Vec<String>>,
    /// See [`OutputConfig::import_alias`]. Deprecated single-output form.
    #[serde(rename = "importAlias")]
    pub import_alias: Option<String>,
    /// Every output this instance resolves against. Takes precedence over the
    /// single-output fields above.
    #[serde(rename = "outputs")]
    pub outputs: Option<Vec<OutputConfig>>,
    /// File extension to append to generated import paths
    /// Options: "none" (default), "ts", "dts", "js"
    #[serde(rename = "emitExtensions", default)]
    pub emit_extensions: EmitExtensions,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ManifestEntry {
    pub source: String,
    pub path: String,
    pub name: String,
}

/// One `OutputConfig` with its manifest indexed for lookup.
struct ResolvedOutput {
    output_dir: PathBuf,
    import_alias: Option<String>,
    package_root: Option<PathBuf>,
    import_paths: Vec<String>,
    /// Normalized document source -> entry
    manifest: HashMap<String, ManifestEntry>,
    /// Document name -> entry
    name_to_entry: HashMap<String, ManifestEntry>,
}

pub struct TransformVisitor {
    outputs: Vec<ResolvedOutput>,
    current_file: Option<PathBuf>,
    new_imports: HashMap<String, (String, String)>, // local_name -> (imported_name, source_path)
    /// Local ids bound to `graphql`/`gql`, mapped to the output whose entrypoint
    /// they came from, so a call resolves against that project's manifest.
    graphql_ids: HashMap<Id, usize>,
    emit_extensions: EmitExtensions,
    existing_names: std::collections::HashSet<String>,
    /// (owning output, document name) -> local name in this module. Keyed by the
    /// output too: two projects may legitimately export the same document name,
    /// and each needs its own binding.
    document_name_to_local_name: HashMap<(usize, String), String>,
    id_renames: HashMap<Id, String>,
}

fn normalize(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
}

fn strip_script_extension(s: &str) -> String {
    s.strip_suffix(".d.ts")
        .or_else(|| s.strip_suffix(".tsx"))
        .or_else(|| s.strip_suffix(".jsx"))
        .or_else(|| s.strip_suffix(".mjs"))
        .or_else(|| s.strip_suffix(".cjs"))
        .or_else(|| s.strip_suffix(".ts"))
        .or_else(|| s.strip_suffix(".js"))
        .unwrap_or(s)
        .to_string()
}

fn strip_script_extension_path(path: &Path) -> PathBuf {
    PathBuf::from(strip_script_extension(&path.to_string_lossy()))
}

#[derive(Clone)]
struct DynamicImportRequest {
    imported_name: String,
    source_path: String,
}

fn get_static_string(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Lit(Lit::Str(s)) => Some(s.value.as_str().unwrap_or("").to_string()),
        Expr::Tpl(tpl) if tpl.exprs.is_empty() && tpl.quasis.len() == 1 => tpl.quasis[0]
            .cooked
            .as_ref()
            .and_then(|c| c.as_str().map(|value| value.to_string()))
            .or_else(|| Some(tpl.quasis[0].raw.as_str().to_string())),
        _ => None,
    }
}

fn get_dynamic_import_source(expr: &Expr) -> Option<(String, bool)> {
    match expr {
        Expr::Await(await_expr) => {
            let Expr::Call(call) = &*await_expr.arg else {
                return None;
            };

            if !matches!(call.callee, Callee::Import(_)) {
                return None;
            }

            call.args
                .first()
                .and_then(|arg| get_static_string(arg.expr.as_ref()))
                .map(|source| (source, true))
        }
        Expr::Call(call) if matches!(call.callee, Callee::Import(_)) => call
            .args
            .first()
            .and_then(|arg| get_static_string(arg.expr.as_ref()))
            .map(|source| (source, false)),
        _ => None,
    }
}

fn parse_expression(source: &str) -> Expr {
    let cm = std::sync::Arc::<swc_core::common::SourceMap>::default();
    let fm = cm.new_source_file(
        swc_core::common::FileName::Custom("graphox_dynamic_import.ts".into()).into(),
        source.to_string(),
    );
    let mut parser = swc_core::ecma::parser::Parser::new(
        swc_core::ecma::parser::Syntax::Typescript(swc_core::ecma::parser::TsSyntax::default()),
        swc_core::ecma::parser::StringInput::from(&*fm),
        None,
    );

    *parser
        .parse_expr()
        .unwrap_or_else(|err| panic!("failed to parse generated expression `{source}`: {err:?}"))
}

impl TransformVisitor {
    pub fn new(config: &Config, current_file: Option<String>) -> Self {
        // `outputs` wins; otherwise fold the deprecated single-output fields into
        // a one-element list so everything below has one shape to work with.
        let output_configs: Vec<OutputConfig> = match &config.outputs {
            Some(outputs) if !outputs.is_empty() => outputs.clone(),
            _ => vec![OutputConfig {
                output_dir: config.output_dir.clone(),
                import_alias: config.import_alias.clone(),
                manifest_path: config.manifest_path.clone(),
                manifest_data: config.manifest_data.clone(),
                graphql_import_paths: config.graphql_import_paths.clone(),
                package_root: None,
            }],
        };

        let outputs: Vec<ResolvedOutput> = output_configs
            .iter()
            .map(|output| {
                let entries = if let Some(data) = &output.manifest_data {
                    data.clone()
                } else if let Some(path) = &output.manifest_path {
                    let manifest_content =
                        std::fs::read_to_string(path).unwrap_or_else(|_| "[]".to_string());
                    serde_json::from_str(&manifest_content).unwrap_or_default()
                } else {
                    vec![]
                };

                let mut manifest = HashMap::new();
                let mut name_to_entry = HashMap::new();
                for entry in entries {
                    manifest
                        .entry(normalize(&entry.source))
                        .or_insert_with(|| entry.clone());
                    name_to_entry.insert(entry.name.clone(), entry);
                }

                ResolvedOutput {
                    output_dir: PathBuf::from(&output.output_dir),
                    import_alias: output.import_alias.clone(),
                    package_root: output.package_root.as_ref().map(PathBuf::from),
                    import_paths: output.graphql_import_paths.clone().unwrap_or_default(),
                    manifest,
                    name_to_entry,
                }
            })
            .collect();

        Self {
            outputs,
            current_file: current_file.map(PathBuf::from),
            new_imports: HashMap::new(),
            graphql_ids: HashMap::new(),
            emit_extensions: config.emit_extensions.clone(),
            existing_names: std::collections::HashSet::new(),
            document_name_to_local_name: HashMap::new(),
            id_renames: HashMap::new(),
        }
    }

    fn get_local_name(&mut self, output_idx: usize, document_name: &str) -> String {
        let key = (output_idx, document_name.to_string());
        if let Some(local_name) = self.document_name_to_local_name.get(&key) {
            return local_name.clone();
        }

        let mut unique_name = document_name.to_string();
        if self.is_local_name_taken(&unique_name) {
            let mut i = 1;
            while self.is_local_name_taken(&format!("{}{}", document_name, i)) {
                i += 1;
            }
            unique_name = format!("{}{}", document_name, i);
        }

        self.document_name_to_local_name
            .insert(key, unique_name.clone());
        unique_name
    }

    /// A local name is unavailable if the module already uses it for something of
    /// its own, or if an import we are emitting has claimed it — which is how the
    /// same document name owned by two different outputs stays two bindings.
    fn is_local_name_taken(&self, name: &str) -> bool {
        self.existing_names.contains(name) || self.new_imports.contains_key(name)
    }

    /// Where to import a document from, given the output that owns it.
    ///
    /// Within the current file's own output that is a relative path, as before.
    /// Across outputs a relative path would reach into another package past its
    /// subpath exports, so the import goes through that output's configured
    /// alias instead — which is why the alias is required for cross-project use.
    fn get_import_path(&self, output_idx: usize, codegen_rel_path: &str) -> String {
        let output = &self.outputs[output_idx];
        let in_same_package = match (&output.package_root, &self.current_file) {
            (Some(root), Some(file)) => file.starts_with(root),
            // No package root configured: keep the single-output behaviour of
            // always emitting a relative path.
            _ => true,
        };

        if !in_same_package {
            let Some(alias) = &output.import_alias else {
                self.fail(format!(
                    "\"{}\" belongs to the output at \"{}\", which has no importAlias. Set one so documents in it can be imported from other projects.",
                    codegen_rel_path,
                    output.output_dir.display()
                ));
            };

            let file = strip_script_extension(codegen_rel_path)
                .trim_start_matches("./")
                .to_string();
            let alias = alias.trim_end_matches('/');
            return format!("{}/{}{}", alias, file, self.emit_extensions.as_str());
        }

        self.get_relative_import_path(output_idx, codegen_rel_path)
    }

    fn get_relative_import_path(&self, output_idx: usize, codegen_rel_path: &str) -> String {
        let mut result = if let Some(current_file) = &self.current_file {
            let codegen_abs_path = self.outputs[output_idx].output_dir.join(codegen_rel_path);
            if let Some(parent) = current_file.parent()
                && let Some(rel_path) = pathdiff::diff_paths(&codegen_abs_path, parent)
            {
                let mut s = rel_path.to_string_lossy().to_string();
                if cfg!(windows) {
                    s = s.replace('\\', "/");
                }
                if !s.starts_with('.') && !s.starts_with('/') {
                    s = format!("./{}", s);
                }
                s
            } else {
                let mut s = codegen_rel_path.to_string();
                if cfg!(windows) {
                    s = s.replace('\\', "/");
                }
                s
            }
        } else {
            let mut s = codegen_rel_path.to_string();
            if cfg!(windows) {
                s = s.replace('\\', "/");
            }
            s
        };

        // Append the emit extension
        result.push_str(self.emit_extensions.as_str());
        result
    }

    fn fail(&self, message: impl AsRef<str>) -> ! {
        if let Some(current_file) = &self.current_file {
            panic!(
                "@graphox/swc-plugin ({}): {}",
                current_file.display(),
                message.as_ref()
            );
        }

        panic!("@graphox/swc-plugin: {}", message.as_ref());
    }

    /// Which output's graphql entrypoint (or index barrel) `src` refers to, if
    /// any. Returns the output index so the caller knows whose manifest to
    /// resolve against and how to write the replacement import.
    fn resolve_graphql_path(&self, src: &str) -> Option<usize> {
        // Normalize incoming src path for comparison on Windows
        let src = if cfg!(windows) {
            src.replace('\\', "/")
        } else {
            src.to_string()
        };
        let src = src.as_str();
        let src_no_ext = strip_script_extension(src);

        // Bare specifiers first: an alias or a configured import path is how a
        // module in another project names someone else's entrypoint.
        for (idx, output) in self.outputs.iter().enumerate() {
            if let Some(alias) = &output.import_alias
                && (src == alias.as_str() || src_no_ext == strip_script_extension(alias))
            {
                return Some(idx);
            }

            for path in &output.import_paths {
                if src == path || src_no_ext == strip_script_extension(path) {
                    return Some(idx);
                }
            }
        }

        let current_file = self.current_file.as_ref()?;
        let parent = current_file.parent()?;

        for (idx, output) in self.outputs.iter().enumerate() {
            if output.output_dir.as_os_str().is_empty() {
                continue;
            }

            let entrypoint_abs_path = output.output_dir.join("graphql");
            let index_abs_path = output.output_dir.join("index");
            let entrypoint_abs_no_ext = strip_script_extension_path(&entrypoint_abs_path);
            let index_abs_no_ext = strip_script_extension_path(&index_abs_path);

            let src_path = Path::new(src);
            let is_absolute_looking = src_path.is_absolute()
                || (cfg!(windows) && (src.starts_with('/') || src.starts_with('\\')));

            let resolved_abs = if src.starts_with('.') {
                Some(parent.join(src_path))
            } else if is_absolute_looking {
                Some(src_path.to_path_buf())
            } else {
                None
            };

            if let Some(resolved_abs) = resolved_abs {
                let resolved_no_ext = strip_script_extension_path(&resolved_abs);
                if resolved_no_ext == entrypoint_abs_no_ext
                    || resolved_no_ext == index_abs_no_ext
                    || resolved_no_ext.join("index") == index_abs_no_ext
                {
                    return Some(idx);
                }
            }

            if let Some(rel_path) = pathdiff::diff_paths(&entrypoint_abs_path, parent)
                && let Some(rel_index_path) = pathdiff::diff_paths(&index_abs_path, parent)
            {
                let mut s = rel_path.to_string_lossy().to_string();
                if cfg!(windows) {
                    s = s.replace('\\', "/");
                }
                if !s.starts_with('.') && !s.starts_with('/') {
                    s = format!("./{}", s);
                }

                let mut s_index = rel_index_path.to_string_lossy().to_string();
                if cfg!(windows) {
                    s_index = s_index.replace('\\', "/");
                }
                if !s_index.starts_with('.') && !s_index.starts_with('/') {
                    s_index = format!("./{}", s_index);
                }

                // Normalize both source and our paths to compare without extensions
                let src_normalized = src_no_ext.as_str();
                let our_normalized = strip_script_extension(&s);
                let our_index_normalized = strip_script_extension(&s_index);

                if src_normalized == our_normalized || src_normalized == our_index_normalized {
                    return Some(idx);
                }

                // Handle directory import resolving to index
                if format!("{}/index", src_normalized) == our_index_normalized {
                    return Some(idx);
                }
            }
        }

        None
    }

    fn is_our_graphql_path(&self, src: &str) -> bool {
        self.resolve_graphql_path(src).is_some()
    }

    /// Raised when a document is imported from a recognised graphql entrypoint
    /// but appears in no configured manifest. The usual cause is that the
    /// project owning the document was never registered with the plugin, so its
    /// entrypoint got cleared with nothing to redirect the import to.
    fn unresolved_document_error(&self, imported_name: &str, source: &str) -> String {
        let configured = self
            .outputs
            .iter()
            .map(|output| output.output_dir.display().to_string())
            .filter(|dir| !dir.is_empty())
            .collect::<Vec<_>>();

        format!(
            "could not rewrite \"{}\" from \"{}\". It is in none of the configured manifests [{}]. Register the outputDir of the project that defines it, or run Graphox codegen.",
            imported_name,
            source,
            configured.join(", ")
        )
    }

    /// Stands in for `graphql`/`gql` once the entrypoint has been emptied.
    ///
    /// Nothing should reach it: a rewritten call site imports the generated
    /// document directly, and a call site left holding a live reference to
    /// `graphql` is a build error already. It is reachable only when the plugin
    /// failed to recognise the specifier some module used to import this
    /// entrypoint — the module is then untouched while the entrypoint it calls is
    /// emptied regardless, because clearing keys on the file path. Returning a
    /// non-document there fails much later, inside whichever client receives it,
    /// with nothing pointing back here.
    fn emptied_entrypoint_stub(&self) -> Expr {
        let file = self
            .current_file
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "this graphql entrypoint".to_string());

        let message = format!(
            "graphox: {} was emptied at build time — its documents are inlined into the \
             generated files — but graphql() was called through it at runtime. \
             @graphox/swc-plugin did not recognise the specifier the calling module used to \
             import this entrypoint, so that module was never rewritten. Add the specifier it \
             imports to graphqlImportPaths for this output.",
            file
        );

        parse_expression(&format!(
            "() => {{ throw new Error({}) }}",
            serde_json::to_string(&message).unwrap()
        ))
    }

    fn star_reexport_error(&self, source: &str) -> String {
        format!(
            "could not rewrite a star re-export of \"{}\". That entrypoint is emptied at build time, so nothing would be left to re-export. Name the documents instead: export {{ SomeDocument }} from \"{}\".",
            source, source
        )
    }

    /// Point `export {{ A, B }} from "<entrypoint>"` at the generated files.
    ///
    /// A re-export binds nothing locally, so there is no renaming to do — only
    /// the source moves. Documents named in one declaration can live in
    /// different generated files, so one declaration may become several; they are
    /// emitted in the order the paths were first named, to keep the output
    /// stable.
    fn rewrite_document_reexport(&self, export: NamedExport) -> Vec<ModuleItem> {
        let src = export.src.as_ref().expect("checked by the caller");
        let source = src.value.as_str().unwrap_or("");
        let output_idx = self
            .resolve_graphql_path(source)
            .expect("checked by the caller");

        let mut order: Vec<String> = Vec::new();
        let mut by_path: HashMap<String, Vec<ExportSpecifier>> = HashMap::new();

        for specifier in export.specifiers {
            let named = match specifier {
                ExportSpecifier::Named(named) => named,
                // `export * as ns from` and `export v from` would both need the
                // emptied entrypoint to still hold the documents.
                _ => self.fail(self.star_reexport_error(source)),
            };

            let orig_name = match &named.orig {
                ModuleExportName::Ident(ident) => ident.sym.as_str(),
                ModuleExportName::Str(s) => s.value.as_str().unwrap_or(""),
                #[cfg(swc_ast_unknown)]
                _ => "",
            };

            // Types are erased before this output runs, and the entrypoint they
            // came from is emptied, so a type-only re-export has nothing left to
            // carry. Dropped, as a type-only import from the entrypoint is.
            if export.type_only || named.is_type_only {
                continue;
            }

            if orig_name == "graphql" || orig_name == "gql" {
                self.fail(format!(
                    "could not re-export \"{}\" from \"{}\". It is replaced at build time and does not exist at runtime.",
                    orig_name, source
                ));
            }

            let Some(entry) = self.outputs[output_idx].name_to_entry.get(orig_name) else {
                self.fail(self.unresolved_document_error(orig_name, source));
            };

            let path = self.get_import_path(output_idx, &entry.path);
            if !by_path.contains_key(&path) {
                order.push(path.clone());
            }
            by_path
                .entry(path)
                .or_default()
                .push(ExportSpecifier::Named(named));
        }

        order
            .into_iter()
            .map(|path| {
                ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(NamedExport {
                    span: export.span,
                    specifiers: by_path.remove(&path).unwrap_or_default(),
                    src: Some(Box::new(Str {
                        span: DUMMY_SP,
                        value: path.into(),
                        raw: None,
                    })),
                    type_only: false,
                    with: None,
                }))
            })
            .collect()
    }

    fn dynamic_import_error(&self, source: &str) -> String {
        format!(
            "could not fully rewrite this dynamic import from \"{}\". Use object destructuring of named documents from the generated graphql entrypoint or split the import by document.",
            source
        )
    }

    fn collect_dynamic_import_requests(
        &self,
        source: &str,
        pattern: &ObjectPat,
    ) -> Vec<DynamicImportRequest> {
        let mut requests = Vec::new();

        for prop in &pattern.props {
            let imported_name = match prop {
                ObjectPatProp::Assign(assign) => assign.key.sym.to_string(),
                ObjectPatProp::KeyValue(key_value) => match &key_value.key {
                    PropName::Ident(ident) => ident.sym.to_string(),
                    PropName::Str(s) => s.value.as_str().unwrap_or("").to_string(),
                    _ => self.fail(self.dynamic_import_error(source)),
                },
                ObjectPatProp::Rest(_) => self.fail(self.dynamic_import_error(source)),
                #[cfg(swc_ast_unknown)]
                _ => self.fail(self.dynamic_import_error(source)),
            };

            if imported_name == "graphql" || imported_name == "gql" {
                self.fail(self.dynamic_import_error(source));
            }

            let output_idx = self.resolve_graphql_path(source).unwrap_or(0);
            let Some(entry) = self.outputs[output_idx].name_to_entry.get(&imported_name) else {
                self.fail(self.unresolved_document_error(&imported_name, source));
            };
            let path = entry.path.clone();

            requests.push(DynamicImportRequest {
                imported_name,
                source_path: self.get_import_path(output_idx, &path),
            });
        }

        requests
    }

    fn rewrite_dynamic_import_expr(
        &self,
        awaited: bool,
        requests: &[DynamicImportRequest],
    ) -> Expr {
        let mut unique_paths = Vec::<String>::new();
        let mut module_name_by_path = HashMap::<String, String>::new();

        for request in requests {
            if !module_name_by_path.contains_key(&request.source_path) {
                let module_name = format!("_graphoxModule{}", unique_paths.len());
                module_name_by_path.insert(request.source_path.clone(), module_name);
                unique_paths.push(request.source_path.clone());
            }
        }

        let base_expr = if unique_paths.len() == 1 {
            parse_expression(&format!(
                "import({})",
                serde_json::to_string(&unique_paths[0]).unwrap()
            ))
        } else {
            let imports = unique_paths
                .iter()
                .map(|path| format!("import({})", serde_json::to_string(path).unwrap()))
                .collect::<Vec<_>>()
                .join(", ");
            let module_names = unique_paths
                .iter()
                .map(|path| module_name_by_path.get(path).unwrap().clone())
                .collect::<Vec<_>>()
                .join(", ");
            let object_properties = requests
                .iter()
                .map(|request| {
                    let module_name = module_name_by_path.get(&request.source_path).unwrap();
                    format!(
                        "{}: {}.{}",
                        request.imported_name, module_name, request.imported_name
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");

            parse_expression(&format!(
                "Promise.all([{}]).then(([{}]) => ({{ {} }}))",
                imports, module_names, object_properties
            ))
        };

        if awaited {
            Expr::Await(AwaitExpr {
                span: DUMMY_SP,
                arg: Box::new(base_expr),
            })
        } else {
            base_expr
        }
    }
}

impl VisitMut for TransformVisitor {
    fn visit_mut_module(&mut self, n: &mut Module) {
        // If we are processing the entrypoint itself, clear it
        if let Some(current_file) = &self.current_file {
            let current_file_normalized = current_file.with_extension("");
            let is_entrypoint = self.outputs.iter().any(|output| {
                !output.output_dir.as_os_str().is_empty()
                    && current_file_normalized == output.output_dir.join("graphql")
            });
            if is_entrypoint {
                n.body.clear();
                // Add empty exports to satisfy build tools
                n.body
                    .push(ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl {
                        span: DUMMY_SP,
                        decl: Decl::Var(Box::new(VarDecl {
                            span: DUMMY_SP,
                            ctxt: SyntaxContext::empty(),
                            kind: VarDeclKind::Const,
                            declare: false,
                            decls: vec![VarDeclarator {
                                span: DUMMY_SP,
                                name: Pat::Ident(BindingIdent {
                                    id: Ident {
                                        sym: "graphql".into(),
                                        span: DUMMY_SP,
                                        ctxt: SyntaxContext::empty(),
                                        optional: false,
                                    },
                                    type_ann: None,
                                }),
                                init: Some(Box::new(self.emptied_entrypoint_stub())),
                                definite: false,
                            }],
                        })),
                    })));
                n.body
                    .push(ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(ExportDecl {
                        span: DUMMY_SP,
                        decl: Decl::Var(Box::new(VarDecl {
                            span: DUMMY_SP,
                            ctxt: SyntaxContext::empty(),
                            kind: VarDeclKind::Const,
                            declare: false,
                            decls: vec![VarDeclarator {
                                span: DUMMY_SP,
                                name: Pat::Ident(BindingIdent {
                                    id: Ident {
                                        sym: "gql".into(),
                                        span: DUMMY_SP,
                                        ctxt: SyntaxContext::empty(),
                                        optional: false,
                                    },
                                    type_ann: None,
                                }),
                                init: Some(Box::new(Expr::Ident(Ident {
                                    sym: "graphql".into(),
                                    span: DUMMY_SP,
                                    ctxt: SyntaxContext::empty(),
                                    optional: false,
                                }))),
                                definite: false,
                            }],
                        })),
                    })));
                return;
            }
        }

        let mut our_import_ids = std::collections::HashSet::new();

        // First pass: identify imports from our graphql.ts or index.ts
        for item in &n.body {
            if let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item {
                let src = import.src.value.as_str().unwrap_or("");
                if let Some(output_idx) = self.resolve_graphql_path(src) {
                    let import_is_type_only = import.type_only;
                    for specifier in &import.specifiers {
                        match specifier {
                            ImportSpecifier::Named(named) => {
                                let local_name = named.local.sym.as_str();
                                let imported_name = named
                                    .imported
                                    .as_ref()
                                    .map(|i| match i {
                                        ModuleExportName::Ident(id) => id.sym.as_str(),
                                        ModuleExportName::Str(s) => s.value.as_str().unwrap_or(""),
                                        #[cfg(swc_ast_unknown)]
                                        _ => "",
                                    })
                                    .unwrap_or(local_name);

                                let specifier_is_type_only =
                                    import_is_type_only || named.is_type_only;

                                our_import_ids.insert(named.local.to_id());

                                if imported_name == "graphql" || imported_name == "gql" {
                                    self.graphql_ids.insert(named.local.to_id(), output_idx);
                                } else if self.outputs[output_idx]
                                    .name_to_entry
                                    .contains_key(imported_name)
                                    && !specifier_is_type_only
                                {
                                    // Only track non-type-only imports of document names
                                    let entry_path = self.outputs[output_idx]
                                        .name_to_entry
                                        .get(imported_name)
                                        .unwrap()
                                        .path
                                        .clone();
                                    let rel_path = self.get_import_path(output_idx, &entry_path);

                                    let target_local_name = if local_name != imported_name {
                                        // Aliased, keep it and register it
                                        self.document_name_to_local_name.insert(
                                            (output_idx, imported_name.to_string()),
                                            local_name.to_string(),
                                        );
                                        local_name.to_string()
                                    } else {
                                        self.get_local_name(output_idx, imported_name)
                                    };

                                    self.new_imports.insert(
                                        target_local_name.clone(),
                                        (imported_name.to_string(), rel_path),
                                    );

                                    // Always record the binding, even when the name
                                    // is unchanged: the import we emit is a fresh
                                    // identifier, so every reference has to be
                                    // moved onto it. See `visit_mut_ident`.
                                    self.id_renames
                                        .insert(named.local.to_id(), target_local_name);
                                } else if !specifier_is_type_only {
                                    self.fail(self.unresolved_document_error(imported_name, src));
                                }
                            }
                            ImportSpecifier::Default(_) if !import_is_type_only => {
                                self.fail(format!(
                                    "could not fully rewrite this default import from \"{}\". Only named document imports and graphql/gql are supported.",
                                    src
                                ));
                            }
                            ImportSpecifier::Namespace(_) if !import_is_type_only => {
                                self.fail(format!(
                                    "could not fully rewrite this namespace import from \"{}\". Only named document imports and graphql/gql are supported.",
                                    src
                                ));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // Collect all names EXCEPT those from our imports
        let mut colliding_names = std::collections::HashSet::new();
        struct IdCollector<'a, 'b>(
            &'a mut std::collections::HashSet<String>,
            &'b std::collections::HashSet<Id>,
        );
        impl Visit for IdCollector<'_, '_> {
            fn visit_ident(&mut self, n: &Ident) {
                if !self.1.contains(&n.to_id()) {
                    self.0.insert(n.sym.to_string());
                }
            }
        }
        n.visit_with(&mut IdCollector(&mut colliding_names, &our_import_ids));
        self.existing_names = colliding_names;

        n.visit_mut_children_with(self);

        let mut validator = GraphqlUsageValidator {
            visitor: self,
            error: None,
        };
        n.visit_with(&mut validator);
        if let Some(error) = validator.error {
            self.fail(error);
        }

        // Drop the imports we rewrote and redirect any re-export of a document at
        // the generated file, keeping the position of what we replace: the
        // entrypoint is emptied in its own compilation, so anything still
        // pointing at it resolves to nothing.
        let mut rebuilt: Vec<ModuleItem> = Vec::with_capacity(n.body.len());
        let mut insert_at = None;

        for item in std::mem::take(&mut n.body) {
            match item {
                ModuleItem::ModuleDecl(ModuleDecl::Import(import))
                    if self.is_our_graphql_path(import.src.value.as_str().unwrap_or("")) =>
                {
                    // Our imports are replaced by the ones collected below, which
                    // go in where the first of them stood rather than at the top —
                    // ahead of a side-effect import is a different module.
                    insert_at.get_or_insert(rebuilt.len());
                }
                ModuleItem::ModuleDecl(ModuleDecl::ExportNamed(export))
                    if export.src.as_ref().is_some_and(|src| {
                        self.is_our_graphql_path(src.value.as_str().unwrap_or(""))
                    }) =>
                {
                    rebuilt.extend(self.rewrite_document_reexport(export));
                }
                ModuleItem::ModuleDecl(ModuleDecl::ExportAll(export))
                    if self.is_our_graphql_path(export.src.value.as_str().unwrap_or("")) =>
                {
                    self.fail(self.star_reexport_error(export.src.value.as_str().unwrap_or("")));
                }
                other => rebuilt.push(other),
            }
        }
        n.body = rebuilt;

        // Add the new imports where the ones they replace stood (sorted
        // alphabetically by local name)
        let insert_at = insert_at.unwrap_or(0);
        let mut imports: Vec<_> = self.new_imports.iter().collect();
        imports.sort_by(|a, b| a.0.cmp(b.0));

        for (i, (local_name, (imported_name, source_path))) in imports.into_iter().enumerate() {
            let imported = if local_name != imported_name {
                Some(ModuleExportName::Ident(Ident::new(
                    imported_name.clone().into(),
                    DUMMY_SP,
                    Default::default(),
                )))
            } else {
                None
            };

            n.body.insert(
                insert_at + i,
                ModuleItem::ModuleDecl(ModuleDecl::Import(ImportDecl {
                    span: DUMMY_SP,
                    specifiers: vec![ImportSpecifier::Named(ImportNamedSpecifier {
                        span: DUMMY_SP,
                        local: Ident::new(local_name.clone().into(), DUMMY_SP, Default::default()),
                        imported,
                        is_type_only: false,
                    })],
                    src: Box::new(Str {
                        span: DUMMY_SP,
                        value: source_path.clone().into(),
                        raw: None,
                    }),
                    type_only: false,
                    with: None,
                    phase: Default::default(),
                })),
            );
        }
    }

    fn visit_mut_ident(&mut self, n: &mut Ident) {
        if let Some(new_name) = self.id_renames.get(&n.to_id()) {
            n.sym = new_name.clone().into();
            // The import these identifiers resolved to is about to be removed and
            // replaced by one we synthesize with an empty `SyntaxContext`. Left as
            // they are, the references keep pointing at the old binding, and a
            // later hygiene pass — seeing the same symbol in two contexts — renames
            // our import to `Name1` and leaves the references unbound. Moving them
            // onto the same context keeps binding and references one identifier.
            n.ctxt = SyntaxContext::empty();
        }
    }

    fn visit_mut_named_export(&mut self, n: &mut NamedExport) {
        // A re-export names an export of another module, not a local binding. Its
        // source is redirected as a whole later; renaming the name here would ask
        // the generated file for something it does not export.
        if n.src.is_some() {
            return;
        }

        for specifier in &mut n.specifiers {
            if let ExportSpecifier::Named(named) = specifier
                && let ModuleExportName::Ident(ident) = &mut named.orig
                && let Some(new_name) = self.id_renames.get(&ident.to_id()).cloned()
            {
                // `export { X }` means `export { X as X }`. The binding moves to
                // the name we import it under; the name the module exports must
                // not move with it.
                if named.exported.is_none() && new_name.as_str() != &*ident.sym {
                    named.exported = Some(ModuleExportName::Ident(Ident::new(
                        ident.sym.clone(),
                        DUMMY_SP,
                        SyntaxContext::empty(),
                    )));
                }

                ident.sym = new_name.into();
                ident.ctxt = SyntaxContext::empty();
            }
        }
    }

    fn visit_mut_prop(&mut self, n: &mut Prop) {
        // `{ X }` means `{ X: X }`. Renaming it in place would move the property
        // name along with the value it reads.
        if let Prop::Shorthand(ident) = n
            && let Some(new_name) = self.id_renames.get(&ident.to_id()).cloned()
            && new_name.as_str() != &*ident.sym
        {
            *n = Prop::KeyValue(KeyValueProp {
                key: PropName::Ident(IdentName::new(ident.sym.clone(), ident.span)),
                value: Box::new(Expr::Ident(Ident::new(
                    new_name.into(),
                    ident.span,
                    SyntaxContext::empty(),
                ))),
            });
            return;
        }

        n.visit_mut_children_with(self);
    }

    fn visit_mut_var_declarator(&mut self, n: &mut VarDeclarator) {
        n.name.visit_mut_with(self);

        if let Some(init) = &mut n.init {
            if let Some((source, awaited)) = get_dynamic_import_source(init.as_ref())
                && self.is_our_graphql_path(&source)
            {
                let Pat::Object(pattern) = &n.name else {
                    self.fail(self.dynamic_import_error(&source));
                };

                let requests = self.collect_dynamic_import_requests(&source, pattern);
                if !requests.is_empty() {
                    **init = self.rewrite_dynamic_import_expr(awaited, &requests);
                }
            }

            init.visit_mut_with(self);
        }
    }

    fn visit_mut_expr(&mut self, n: &mut Expr) {
        n.visit_mut_children_with(self);

        if let Expr::Call(call) = n
            && let Callee::Expr(callee_expr) = &call.callee
            && let Expr::Ident(ident) = &**callee_expr
            && let Some(&output_idx) = self.graphql_ids.get(&ident.to_id())
            && let Some(ExprOrSpread { expr, .. }) = call.args.first()
        {
            let source = match &**expr {
                Expr::Tpl(tpl) => {
                    if tpl.quasis.len() == 1 {
                        let s = match &tpl.quasis[0].cooked {
                            Some(c) => c.as_str(),
                            None => Some(tpl.quasis[0].raw.as_str()),
                        };
                        s.map(normalize)
                    } else {
                        None
                    }
                }
                Expr::Lit(Lit::Str(s)) => Some(normalize(s.value.as_str().unwrap_or(""))),
                #[cfg(swc_ast_unknown)]
                _ => None,
                #[cfg(not(swc_ast_unknown))]
                _ => None,
            };

            let Some(source) = source else {
                self.fail(format!(
                    "could not statically analyze this {}() call. Use a single static string/template literal so it can be resolved from the manifest.",
                    ident.sym
                ));
            };

            let Some(entry) = self.outputs[output_idx].manifest.get(&source) else {
                self.fail(format!(
                    "could not find this {}() document in the manifest for \"{}\". Run Graphox codegen and ensure the build is using the correct manifest.",
                    ident.sym,
                    self.outputs[output_idx].output_dir.display()
                ));
            };

            let entry_name = entry.name.clone();
            let entry_path = entry.path.clone();
            let rel_path = self.get_import_path(output_idx, &entry_path);
            let target_local_name = self.get_local_name(output_idx, &entry_name);
            self.new_imports
                .insert(target_local_name.clone(), (entry_name, rel_path));
            *n = Expr::Ident(Ident::new(
                target_local_name.into(),
                DUMMY_SP,
                Default::default(),
            ));
        }
    }
}

struct GraphqlUsageValidator<'a> {
    visitor: &'a TransformVisitor,
    error: Option<String>,
}

impl Visit for GraphqlUsageValidator<'_> {
    /// `export { graphql }` re-exports a binding whose import is about to be
    /// removed. It is not an expression, so the check below never sees it, and
    /// the emitted module exports a name nothing declares.
    fn visit_named_export(&mut self, n: &NamedExport) {
        if self.error.is_some() || n.src.is_some() {
            return;
        }

        for specifier in &n.specifiers {
            if let ExportSpecifier::Named(named) = specifier
                && let ModuleExportName::Ident(ident) = &named.orig
                && self.visitor.graphql_ids.contains_key(&ident.to_id())
            {
                self.error = Some(format!(
                    "left a runtime reference to \"{}\" after rewriting. All Graphox graphql/gql imports must be fully inlined before the import is removed.",
                    ident.sym
                ));
                return;
            }
        }
    }

    fn visit_expr(&mut self, n: &Expr) {
        if self.error.is_some() {
            return;
        }

        if let Expr::Ident(ident) = n
            && self.visitor.graphql_ids.contains_key(&ident.to_id())
        {
            self.error = Some(format!(
                "left a runtime reference to \"{}\" after rewriting. All Graphox graphql/gql imports must be fully inlined before the import is removed.",
                ident.sym
            ));
            return;
        }

        if let Some((source, _)) = get_dynamic_import_source(n)
            && self.visitor.is_our_graphql_path(&source)
        {
            self.error = Some(self.visitor.dynamic_import_error(&source));
            return;
        }

        n.visit_children_with(self);
    }
}

#[plugin_transform]
pub fn process_transform(
    mut program: Program,
    _metadata: TransformPluginProgramMetadata,
) -> Program {
    let config_str = _metadata.get_transform_plugin_config();
    let config: Config = serde_json::from_str(&config_str.unwrap_or_else(|| "{}".to_string()))
        .unwrap_or_else(|_| Config::default());

    let current_file = _metadata
        .get_context(&swc_core::plugin::metadata::TransformPluginMetadataContextKind::Filename);
    let mut visitor = TransformVisitor::new(&config, current_file);
    program.visit_mut_with(&mut visitor);
    program
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use swc_core::common::{FileName, SourceMap};
    use swc_core::ecma::codegen::{Emitter, text_writer::JsWriter};
    use swc_core::ecma::parser::{Parser, StringInput, Syntax, TsSyntax};
    use swc_core::ecma::visit::VisitMutWith;

    fn transform(source: &str, config: Config, filename: &str) -> String {
        let cm = Arc::<SourceMap>::default();
        let fm = cm.new_source_file(FileName::Custom(filename.into()).into(), source.to_string());

        let mut parser = Parser::new(
            Syntax::Typescript(TsSyntax::default()),
            StringInput::from(&*fm),
            None,
        );
        let mut module = parser.parse_module().expect("Failed to parse module");

        let mut visitor = TransformVisitor::new(&config, Some(filename.to_string()));
        module.visit_mut_with(&mut visitor);

        let mut buf = vec![];
        {
            let mut emitter = Emitter {
                cfg: Default::default(),
                cm: cm.clone(),
                comments: None,
                wr: Box::new(JsWriter::new(cm.clone(), "\n", &mut buf, None)),
            };
            emitter.emit_module(&module).unwrap();
        }

        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn test_visitor_basic() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: ".".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { graphql } from './graphql'; const q = graphql(`query { me { id } }`);",
            config,
            "test.ts",
        );

        assert!(output.contains("import { MyQueryDocument } from \"./query.codegen\";"));
        assert!(output.contains("const q = MyQueryDocument;"));
        assert!(!output.contains("import { MyQueryDocument } from \"./graphql\""));
    }

    #[test]
    fn test_emit_extensions_ts() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: ".".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::Ts,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { graphql } from './graphql'; const q = graphql(`query { me { id } }`);",
            config,
            "test.ts",
        );

        assert!(output.contains("import { MyQueryDocument } from \"./query.codegen.ts\";"));
    }

    #[test]
    fn test_emit_extensions_js() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: ".".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::Js,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { graphql } from './graphql'; const q = graphql(`query { me { id } }`);",
            config,
            "test.ts",
        );

        assert!(output.contains("import { MyQueryDocument } from \"./query.codegen.js\";"));
    }

    #[test]
    fn test_emit_extensions_dts() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: ".".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::Dts,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { graphql } from './graphql'; const q = graphql(`query { me { id } }`);",
            config,
            "test.ts",
        );

        assert!(output.contains("import { MyQueryDocument } from \"./query.codegen.d.ts\";"));
    }

    #[test]
    fn test_visitor_multiple_calls() {
        let manifest = vec![
            ManifestEntry {
                source: "query GetMe { me { id } }".to_string(),
                path: "./me.codegen".to_string(),
                name: "GetMeDocument".to_string(),
            },
            ManifestEntry {
                source: "query GetOther { other { id } }".to_string(),
                path: "./other.codegen".to_string(),
                name: "GetOtherDocument".to_string(),
            },
        ];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: ".".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let source = r#"
            import { graphql } from './graphql';
            const q1 = graphql(`query GetMe { me { id } }`);
            const q2 = graphql(`query GetOther { other { id } }`);
        "#;

        let output = transform(source, config, "test.ts");

        assert!(output.contains("import { GetMeDocument } from \"./me.codegen\";"));
        assert!(output.contains("import { GetOtherDocument } from \"./other.codegen\";"));
        assert!(output.contains("const q1 = GetMeDocument;"));
        assert!(output.contains("const q2 = GetOtherDocument;"));
        assert!(!output.contains("import { graphql }"));
    }

    #[test]
    #[should_panic(expected = "could not rewrite \"other\"")]
    fn test_visitor_rejects_unrewritable_value_imports() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: ".".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        transform(
            "import { graphql, other } from './graphql'; const q = graphql(`query { me { id } }`);",
            config,
            "test.ts",
        );
    }

    #[test]
    #[should_panic(expected = "could not find this graphql() document in the manifest")]
    fn test_visitor_rejects_missing_manifest_entries() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: ".".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        transform(
            "import { graphql } from './graphql'; const q = graphql(`query Missing { me { id } }`);",
            config,
            "test.ts",
        );
    }

    #[test]
    #[should_panic(expected = "could not statically analyze this graphql() call")]
    fn test_visitor_rejects_dynamic_graphql_calls() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: ".".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let source = r#"
            import { graphql } from './graphql';
            const query = 'query { me { id } }';
            const q = graphql(query);
        "#;

        transform(source, config, "test.ts");
    }

    #[test]
    #[should_panic(expected = "left a runtime reference to \"graphql\"")]
    fn test_visitor_rejects_remaining_graphql_references() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: ".".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let source = r#"
            import { graphql } from './graphql';
            const tag = graphql;
            console.log(tag);
        "#;

        transform(source, config, "test.ts");
    }

    #[test]
    fn test_visitor_relative_paths() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./src/query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        // Absolute paths for testing pathdiff
        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: "/root/gen".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { graphql } from '../gen/graphql'; const q = graphql(`query { me { id } }`);",
            config,
            "/root/app/test.ts",
        );

        // /root/gen/src/query.codegen relative to /root/app/
        // should be ../gen/src/query.codegen
        assert!(output.contains("import { MyQueryDocument } from \"../gen/src/query.codegen\";"));
    }

    #[test]
    fn test_visitor_relative_paths_with_extension() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./src/query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: "/root/gen".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::Js,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { graphql } from '../gen/graphql'; const q = graphql(`query { me { id } }`);",
            config,
            "/root/app/test.ts",
        );

        assert!(
            output.contains("import { MyQueryDocument } from \"../gen/src/query.codegen.js\";")
        );
    }

    #[test]
    fn test_visitor_gql_tag() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: ".".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { gql } from './graphql'; const q = gql(`query { me { id } }`);",
            config,
            "test.ts",
        );

        assert!(output.contains("import { MyQueryDocument } from \"./query.codegen\";"));
        assert!(output.contains("const q = MyQueryDocument;"));
        assert!(!output.contains("import { gql }"));
    }

    #[test]
    fn test_normalization() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: ".".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        // Source has different whitespace
        let output = transform(
            "import { graphql } from './graphql'; const q = graphql(`query {
                me {
                    id
                }
            }`);",
            config,
            "test.ts",
        );

        assert!(output.contains("const q = MyQueryDocument;"));
    }

    #[test]
    fn test_normalization_fn() {
        assert_eq!(normalize("  query  { me { id } }  "), "query{me{id}}");
        assert_eq!(normalize("query{\nme{\nid\n}\n}"), "query{me{id}}");
    }

    #[test]
    fn test_other_graphql_import() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: ".".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { graphql } from 'other-lib'; const q = graphql(`query { me { id } }`);",
            config,
            "test.ts",
        );

        // Should NOT be transformed because it's from other-lib
        assert!(output.contains("import { graphql } from 'other-lib';"));
        assert!(output.contains("graphql(`query { me { id } }`)"));
        assert!(!output.contains("MyQueryDocument"));
    }

    #[test]
    fn test_subpath_imports() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: ".".to_string(),
            graphql_import_paths: Some(vec!["#graphql/graphql".to_string()]),
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { graphql } from '#graphql/graphql'; const q = graphql(`query { me { id } }`);",
            config,
            "test.ts",
        );

        assert!(output.contains("import { MyQueryDocument } from \"./query.codegen\";"));
        assert!(output.contains("const q = MyQueryDocument;"));
        assert!(!output.contains("import { graphql }"));
    }

    #[test]
    fn test_does_not_transform_unrelated_aliases_containing_graphql() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: ".".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { graphql } from '#app/graphql/gql'; const q = graphql(`query { me { id } }`);",
            config,
            "test.ts",
        );

        assert!(output.contains("import { graphql } from '#app/graphql/gql';"));
        assert!(output.contains("graphql(`query { me { id } }`)"));
        assert!(!output.contains("MyQueryDocument"));
    }

    #[test]
    fn test_resolves_absolute_import_paths_against_output_dir() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: "/root/gen".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { graphql } from '/root/gen/graphql.ts'; const q = graphql(`query { me { id } }`);",
            config,
            "/root/app/test.ts",
        );

        assert!(output.contains("import { MyQueryDocument } from \"../gen/query.codegen\";"));
        assert!(output.contains("const q = MyQueryDocument;"));
        assert!(!output.contains("from '/root/gen/graphql.ts'"));
    }

    #[test]
    fn test_explicit_import_path() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: ".".to_string(),
            graphql_import_paths: Some(vec!["@app/gql-entrypoint".to_string()]),
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { graphql } from '@app/gql-entrypoint'; const q = graphql(`query { me { id } }`);",
            config,
            "test.ts",
        );

        assert!(output.contains("import { MyQueryDocument } from \"./query.codegen\";"));
        assert!(output.contains("const q = MyQueryDocument;"));
        assert!(!output.contains("import { graphql }"));
    }

    #[test]
    fn test_clear_entrypoint() {
        let config = Config {
            manifest_path: None,
            manifest_data: None,
            output_dir: "/root/gen".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "export const graphql = () => { /* big map */ }; export const gql = graphql;",
            config,
            "/root/gen/graphql.ts",
        );

        // The documents are gone, and what is left names the problem if anything
        // still calls it — a non-document would surface far from here.
        assert!(
            output.contains("export const graphql = ()=>{"),
            "got:\n{output}"
        );
        assert!(output.contains("throw new Error("), "got:\n{output}");
        assert!(output.contains("/root/gen/graphql.ts"), "got:\n{output}");
        assert!(output.contains("export const gql = graphql"));
        assert!(!output.contains("big map"));
    }

    #[test]
    fn test_rewrites_dynamic_import_destructuring_from_graphql_js() {
        let manifest = vec![ManifestEntry {
            source: "mutation CreateCart { createCart { id } }".to_string(),
            path: "./CreateCartMutation.codegen".to_string(),
            name: "CreateCartDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: "/root/gen".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            r#"
            async function load() {
                const { CreateCartDocument } = await import("../gen/graphql.js");
                return CreateCartDocument;
            }
            "#,
            config,
            "/root/app/TokenManager.ts",
        );

        assert!(output.contains(
            "const { CreateCartDocument } = await import(\"../gen/CreateCartMutation.codegen\");"
        ));
        assert!(!output.contains("graphql.js"));
    }

    #[test]
    fn test_rewrites_multi_document_dynamic_imports() {
        let manifest = vec![
            ManifestEntry {
                source: "query GetUser { user { id } }".to_string(),
                path: "./user.codegen".to_string(),
                name: "GetUserDocument".to_string(),
            },
            ManifestEntry {
                source: "query GetPost { post { id } }".to_string(),
                path: "./post.codegen".to_string(),
                name: "GetPostDocument".to_string(),
            },
        ];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: "./gen".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            r#"
            async function load() {
                const { GetUserDocument, GetPostDocument } = await import("./gen/graphql.js");
                return [GetUserDocument, GetPostDocument];
            }
            "#,
            config,
            "test.ts",
        );

        assert!(output.contains("import(\"./gen/user.codegen\")"));
        assert!(output.contains("import(\"./gen/post.codegen\")"));
        assert!(output.contains("GetUserDocument: _graphoxModule0.GetUserDocument"));
        assert!(output.contains("GetPostDocument: _graphoxModule1.GetPostDocument"));
        assert!(!output.contains("graphql.js"));
    }

    #[test]
    #[should_panic(
        expected = "could not fully rewrite this dynamic import from \"./gen/graphql.js\""
    )]
    fn test_rejects_unsupported_dynamic_import_namespaces() {
        let manifest = vec![ManifestEntry {
            source: "query GetUser { user { id } }".to_string(),
            path: "./user.codegen".to_string(),
            name: "GetUserDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: "./gen".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        transform(
            r#"
            async function load() {
                const docs = await import("./gen/graphql.js");
                return docs.GetUserDocument;
            }
            "#,
            config,
            "test.ts",
        );
    }

    #[test]
    fn test_document_name_import_from_directory() {
        let manifest = vec![ManifestEntry {
            source: "query GetUser { user { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "GetUserQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: "/root/gen".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { GetUserQueryDocument } from '../gen'; const doc = GetUserQueryDocument;",
            config,
            "/root/app/test.ts",
        );

        assert!(output.contains("import { GetUserQueryDocument } from \"../gen/query.codegen\";"));
        assert!(!output.contains("from '../gen'"));
        assert!(output.contains("const doc = GetUserQueryDocument;"));
    }

    #[test]
    fn test_document_name_import_multiple() {
        let manifest = vec![
            ManifestEntry {
                source: "query GetUser { user { id } }".to_string(),
                path: "./user.codegen".to_string(),
                name: "GetUserQueryDocument".to_string(),
            },
            ManifestEntry {
                source: "query GetPost { post { title } }".to_string(),
                path: "./post.codegen".to_string(),
                name: "GetPostQueryDocument".to_string(),
            },
        ];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: "/root/gen".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { GetUserQueryDocument, GetPostQueryDocument } from '../gen/graphql';",
            config,
            "/root/app/test.ts",
        );

        assert!(output.contains("import { GetUserQueryDocument } from \"../gen/user.codegen\";"));
        assert!(output.contains("import { GetPostQueryDocument } from \"../gen/post.codegen\";"));
        assert!(!output.contains("from '../gen/graphql'"));
    }

    #[test]
    #[should_panic(expected = "could not rewrite \"OtherType\"")]
    fn test_document_name_mixed_imports() {
        let manifest = vec![ManifestEntry {
            source: "query GetUser { user { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "GetUserQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: "/root/gen".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        transform(
            "import { GetUserQueryDocument, OtherType } from '../gen/graphql';",
            config,
            "/root/app/test.ts",
        );
    }

    #[test]
    fn test_document_name_type_only_import_removed() {
        // Type-only imports are removed entirely since they don't exist in minified JS
        let manifest = vec![ManifestEntry {
            source: "query GetUser { user { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "GetUserQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: "/root/gen".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import type { GetUserQueryDocument } from '../gen/graphql';",
            config,
            "/root/app/test.ts",
        );

        // Type-only imports are removed entirely
        assert!(!output.contains("GetUserQueryDocument"));
        assert!(!output.contains("from '../gen/graphql'"));
    }

    #[test]
    fn test_document_name_import_from_index() {
        let manifest = vec![ManifestEntry {
            source: "query GetUser { user { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "GetUserQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: "/root/gen".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { GetUserQueryDocument } from '../gen/index'; const doc = GetUserQueryDocument;",
            config,
            "/root/app/test.ts",
        );

        assert!(output.contains("import { GetUserQueryDocument } from \"../gen/query.codegen\";"));
        assert!(!output.contains("from '../gen/index'"));
        assert!(output.contains("const doc = GetUserQueryDocument;"));
    }

    #[test]
    fn test_document_name_with_type_specifier() {
        let manifest = vec![ManifestEntry {
            source: "query GetUser { user { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "GetUserQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: "/root/gen".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { GetUserQueryDocument, type GetUserQuery } from '../gen/graphql';",
            config,
            "/root/app/test.ts",
        );

        // GetUserQueryDocument (non-type) is rewritten
        assert!(output.contains("import { GetUserQueryDocument } from \"../gen/query.codegen\";"));
        // GetUserQuery (inline type) is removed
        assert!(!output.contains("type GetUserQuery"));
        assert!(!output.contains("from '../gen/graphql'"));
    }

    #[test]
    fn test_document_name_and_graphql_function() {
        let manifest = vec![ManifestEntry {
            source: "query GetUser { user { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "GetUserQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: "/root/gen".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { graphql, GetUserQueryDocument } from '../gen/graphql'; const q = graphql(`query GetUser { user { id } }`);",
            config,
            "/root/app/test.ts",
        );

        assert!(output.contains("import { GetUserQueryDocument } from \"../gen/query.codegen\";"));
        assert!(!output.contains("from '../gen/graphql'"));
        assert!(output.contains("const q = GetUserQueryDocument;"));
    }

    #[test]
    fn test_visitor_graphql_import_from_index() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: "/root/gen".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { graphql } from '../gen/index'; const q = graphql(`query { me { id } }`);",
            config,
            "/root/app/test.ts",
        );

        assert!(output.contains("import { MyQueryDocument } from \"../gen/query.codegen\";"));
        assert!(output.contains("const q = MyQueryDocument;"));
        assert!(!output.contains("from '../gen/index'"));
    }

    #[test]
    fn test_visitor_gql_import_from_index() {
        let manifest = vec![ManifestEntry {
            source: "query { me { id } }".to_string(),
            path: "./query.codegen".to_string(),
            name: "MyQueryDocument".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: "/root/gen".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { gql } from '../gen/index'; const q = gql(`query { me { id } }`);",
            config,
            "/root/app/test.ts",
        );

        assert!(output.contains("import { MyQueryDocument } from \"../gen/query.codegen\";"));
        assert!(output.contains("const q = MyQueryDocument;"));
        assert!(!output.contains("from '../gen/index'"));
    }

    #[test]
    fn test_fragment_document_with_emit_extensions() {
        // Test that fragment documents (when generate_ast_for_fragments is enabled) work
        let manifest = vec![
            ManifestEntry {
                source: "query GetUser { user { id } }".to_string(),
                path: "./user.codegen".to_string(),
                name: "GetUserQueryDocument".to_string(),
            },
            ManifestEntry {
                source: "fragment UserFields on User { id name }".to_string(),
                path: "./userFields.codegen".to_string(),
                name: "UserFieldsFragmentDocument".to_string(),
            },
        ];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: "/root/gen".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::Ts,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { GetUserQueryDocument, UserFieldsFragmentDocument } from '../gen/graphql';",
            config,
            "/root/app/test.ts",
        );

        assert!(
            output.contains("import { GetUserQueryDocument } from \"../gen/user.codegen.ts\";")
        );
        assert!(output.contains(
            "import { UserFieldsFragmentDocument } from \"../gen/userFields.codegen.ts\";"
        ));
        assert!(!output.contains("from '../gen/graphql'"));
    }

    #[test]
    fn test_prefers_operation_document_for_shared_source_manifest_entries() {
        let source = r#"
            query MusicRouteQuery($playlistId: ID!, $market: IsoCountry!, $categoryTypes: [String!]) {
              playlist(id: $playlistId) {
                ...SourceViewPlaylist
                ...Playlist_MusicRouteMeta
                ...BrowseCategories
              }
            }

            fragment Playlist_MusicRouteMeta on Playlist {
              id
              permissions
              name
              description
              snapshot
              updatedAt
              ...Displayable
              trackStatistics(market: $market) {
                total
              }
            }

            fragment BrowseCategories on Playlist {
              id
              permissions
              browseCategories(categoryTypes: $categoryTypes) {
                id
                name
                slug
                type
              }
            }
        "#;

        let manifest = vec![
            ManifestEntry {
                source: source.to_string(),
                path: "./music.codegen".to_string(),
                name: "MusicRouteQueryQueryDocument".to_string(),
            },
            ManifestEntry {
                source: source.to_string(),
                path: "./music.codegen".to_string(),
                name: "Playlist_MusicRouteMetaFragmentDoc".to_string(),
            },
            ManifestEntry {
                source: source.to_string(),
                path: "./music.codegen".to_string(),
                name: "BrowseCategoriesFragmentDoc".to_string(),
            },
        ];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: ".".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            &format!(
                "import {{ graphql }} from './graphql'; const MusicRouteQuery = graphql(/* GraphQL */ `{}`);",
                source
            ),
            config,
            "test.ts",
        );

        assert!(
            output.contains("import { MusicRouteQueryQueryDocument } from \"./music.codegen\";")
        );
        assert!(output.contains("const MusicRouteQuery = MusicRouteQueryQueryDocument;"));
        assert!(!output.contains("Playlist_MusicRouteMetaFragmentDoc"));
        assert!(!output.contains("BrowseCategoriesFragmentDoc"));
    }

    #[test]
    fn test_keeps_first_fragment_document_for_shared_source_manifest_entries() {
        let source = r#"
            fragment PlaylistFields on Playlist {
              id
            }

            fragment PlaylistPermissions on Playlist {
              permissions
            }
        "#;

        let manifest = vec![
            ManifestEntry {
                source: source.to_string(),
                path: "./playlist.codegen".to_string(),
                name: "PlaylistFieldsFragmentDoc".to_string(),
            },
            ManifestEntry {
                source: source.to_string(),
                path: "./playlist.codegen".to_string(),
                name: "PlaylistPermissionsFragmentDoc".to_string(),
            },
        ];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: ".".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            &format!(
                "import {{ graphql }} from './graphql'; const PlaylistFields = graphql(/* GraphQL */ `{}`);",
                source
            ),
            config,
            "test.ts",
        );

        assert!(
            output.contains("import { PlaylistFieldsFragmentDoc } from \"./playlist.codegen\";")
        );
        assert!(output.contains("const PlaylistFields = PlaylistFieldsFragmentDoc;"));
        assert!(!output.contains("PlaylistPermissionsFragmentDoc"));
    }

    #[test]
    fn test_collision() {
        let manifest = vec![ManifestEntry {
            source: "query q { me { id } }".to_string(),
            path: "./q.codegen".to_string(),
            name: "q".to_string(),
        }];

        let config = Config {
            manifest_path: None,
            manifest_data: Some(manifest),
            output_dir: ".".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
            import_alias: None,
            outputs: None,
        };

        let output = transform(
            "import { graphql } from './graphql'; const q = graphql(`query q { me { id } }`);",
            config,
            "test.ts",
        );

        // Should NOT be "const q = q;"
        assert!(!output.contains("const q = q;"));
        assert!(output.contains("const q = q1;"));
        assert!(output.contains("import { q as q1 } from \"./q.codegen\";"));
    }
    // --- multi-project outputs -------------------------------------------------

    /// Project A (`base`) owns the fragment; project B (`web`) imports it through
    /// A's public alias. Both are registered in one instance.
    fn two_project_config() -> Config {
        Config {
            outputs: Some(vec![
                OutputConfig {
                    output_dir: "/repo/packages/catalog/graphql".to_string(),
                    import_alias: Some("@example/catalog/graphql".to_string()),
                    package_root: Some("/repo/packages/catalog".to_string()),
                    manifest_data: Some(vec![ManifestEntry {
                        source: "fragment ProductCard on Product { id }".to_string(),
                        path: "./catalog.codegen".to_string(),
                        name: "ProductCardFragmentDoc".to_string(),
                    }]),
                    ..Default::default()
                },
                OutputConfig {
                    output_dir: "/repo/packages/storefront/graphql".to_string(),
                    import_alias: Some("@example/storefront/graphql".to_string()),
                    package_root: Some("/repo/packages/storefront".to_string()),
                    manifest_data: Some(vec![ManifestEntry {
                        source: "query Web { web { id } }".to_string(),
                        path: "./storefront.codegen".to_string(),
                        name: "WebQueryDocument".to_string(),
                    }]),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        }
    }

    #[test]
    fn test_cross_project_document_import_uses_alias() {
        // This is the reported failure: B's generated file imports a document
        // from A's barrel. It must be redirected at A's codegen file, through
        // A's alias rather than a relative path that would reach past A's
        // subpath exports.
        let output = transform(
            "import { ProductCardFragmentDoc } from \"@example/catalog/graphql\";\nconst d = ProductCardFragmentDoc;",
            two_project_config(),
            "/repo/packages/storefront/graphql/storefront.codegen.ts",
        );

        assert!(
            output.contains(
                "import { ProductCardFragmentDoc } from \"@example/catalog/graphql/catalog.codegen\""
            ),
            "got:\n{output}"
        );
        assert!(
            !output.contains("\"@example/catalog/graphql\";"),
            "the barrel import must be gone, got:\n{output}"
        );
    }

    #[test]
    fn test_same_package_import_stays_relative() {
        // A module inside A's own package keeps the relative path; only crossing
        // a package boundary needs the alias.
        let output = transform(
            "import { graphql } from \"./graphql\";\nconst d = graphql(`fragment ProductCard on Product { id }`);",
            two_project_config(),
            "/repo/packages/catalog/graphql/consumer.ts",
        );

        assert!(
            output.contains("from \"./catalog.codegen\""),
            "got:\n{output}"
        );
        assert!(!output.contains("@example"), "got:\n{output}");
    }

    #[test]
    fn test_graphql_call_resolves_against_its_own_entrypoint() {
        // The call resolves through the output its `graphql` symbol was imported
        // from, so two outputs are never ambiguous for the same source text.
        let output = transform(
            "import { graphql } from \"@example/storefront/graphql\";\nconst q = graphql(`query Web { web { id } }`);",
            two_project_config(),
            "/repo/apps/web/app/thing.ts",
        );

        assert!(
            output.contains("@example/storefront/graphql/storefront.codegen"),
            "got:\n{output}"
        );
    }

    #[test]
    fn test_entrypoint_cleared_for_every_configured_output() {
        for entrypoint in [
            "/repo/packages/catalog/graphql/graphql.ts",
            "/repo/packages/storefront/graphql/graphql.ts",
        ] {
            let output = transform(
                "export const documents = { a: 1 }; export const graphql = () => documents;",
                two_project_config(),
                entrypoint,
            );
            assert!(
                !output.contains("export const documents") && !output.contains("a: 1"),
                "{entrypoint} should be cleared, got:\n{output}"
            );
            assert!(
                output.contains("export const graphql = ()=>{")
                    && output.contains("throw new Error("),
                "got:\n{output}"
            );
        }
    }

    #[test]
    #[should_panic(expected = "in none of the configured manifests")]
    fn test_unresolved_document_names_the_configured_outputs() {
        transform(
            "import { SomeOtherDoc } from \"@example/catalog/graphql\";\nconst d = SomeOtherDoc;",
            two_project_config(),
            "/repo/packages/storefront/graphql/storefront.codegen.ts",
        );
    }

    #[test]
    #[should_panic(expected = "has no importAlias")]
    fn test_cross_package_without_alias_is_an_error() {
        let mut config = two_project_config();
        config.outputs.as_mut().unwrap()[0].import_alias = None;
        // Still recognised via graphqlImportPaths, but not rewritable.
        config.outputs.as_mut().unwrap()[0].graphql_import_paths =
            Some(vec!["@example/catalog/graphql".to_string()]);

        transform(
            "import { ProductCardFragmentDoc } from \"@example/catalog/graphql\";\nconst d = ProductCardFragmentDoc;",
            config,
            "/repo/packages/storefront/graphql/storefront.codegen.ts",
        );
    }
    // --- duplicate document names across projects ------------------------------

    /// Two projects that legitimately share document names. `SetPrice` has
    /// byte-identical source in both; `ArchiveItem` differs. Neither is ambiguous:
    /// resolution is scoped to the entrypoint the import came from.
    fn duplicate_name_config() -> Config {
        Config {
            outputs: Some(vec![
                OutputConfig {
                    output_dir: "/repo/apps/web/app/graphql".to_string(),
                    import_alias: Some("@example/web/graphql".to_string()),
                    package_root: Some("/repo/apps/web".to_string()),
                    manifest_data: Some(vec![
                        ManifestEntry {
                            source: "mutation SetPrice { setPrice { id } }".to_string(),
                            path: "./web.codegen".to_string(),
                            name: "SetPriceMutationDocument".to_string(),
                        },
                        ManifestEntry {
                            source: "mutation ArchiveItem { archiveItem { id } }".to_string(),
                            path: "./web.codegen".to_string(),
                            name: "ArchiveItemMutationDocument".to_string(),
                        },
                    ]),
                    ..Default::default()
                },
                OutputConfig {
                    output_dir: "/repo/packages/checkout/graphql".to_string(),
                    import_alias: Some("@example/checkout/graphql".to_string()),
                    package_root: Some("/repo/packages/checkout".to_string()),
                    manifest_data: Some(vec![
                        ManifestEntry {
                            source: "mutation SetPrice { setPrice { id } }".to_string(),
                            path: "./checkout.codegen".to_string(),
                            name: "SetPriceMutationDocument".to_string(),
                        },
                        ManifestEntry {
                            source: "mutation ArchiveItem { archiveItem(remote: true) { id } }"
                                .to_string(),
                            path: "./checkout.codegen".to_string(),
                            name: "ArchiveItemMutationDocument".to_string(),
                        },
                    ]),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        }
    }

    #[test]
    fn test_identical_source_in_two_projects_resolves_per_entrypoint() {
        // Byte-identical source in both manifests. The call resolves through the
        // entrypoint its `graphql` symbol came from, so each side gets its own.
        let web = transform(
            "import { graphql } from \"./graphql\";\nconst m = graphql(`mutation SetPrice { setPrice { id } }`);",
            duplicate_name_config(),
            "/repo/apps/web/app/graphql/consumer.ts",
        );
        assert!(web.contains("./web.codegen"), "got:\n{web}");
        assert!(!web.contains("checkout.codegen"), "got:\n{web}");

        let checkout = transform(
            "import { graphql } from \"./graphql\";\nconst m = graphql(`mutation SetPrice { setPrice { id } }`);",
            duplicate_name_config(),
            "/repo/packages/checkout/graphql/consumer.ts",
        );
        assert!(checkout.contains("./checkout.codegen"), "got:\n{checkout}");
        assert!(!checkout.contains("web.codegen"), "got:\n{checkout}");
    }

    #[test]
    fn test_duplicate_document_name_resolves_per_entrypoint() {
        // Same name, different source. A named import resolves in the manifest of
        // whichever output the specifier matched. This module is inside the
        // business package, so its own documents come in by relative path.
        let web = transform(
            "import { ArchiveItemMutationDocument } from \"@example/web/graphql\";\nconst d = ArchiveItemMutationDocument;",
            duplicate_name_config(),
            "/repo/apps/web/app/thing.ts",
        );
        assert!(web.contains("./graphql/web.codegen"), "got:\n{web}");
        assert!(!web.contains("checkout.codegen"), "got:\n{web}");

        let checkout = transform(
            "import { ArchiveItemMutationDocument } from \"@example/checkout/graphql\";\nconst d = ArchiveItemMutationDocument;",
            duplicate_name_config(),
            "/repo/apps/web/app/thing.ts",
        );
        assert!(
            checkout.contains("@example/checkout/graphql/checkout.codegen"),
            "got:\n{checkout}"
        );
        assert!(!checkout.contains("web.codegen"), "got:\n{checkout}");
    }

    #[test]
    fn test_both_duplicated_documents_in_one_module() {
        // The sharpest case: one module pulls the same name from both projects.
        let output = transform(
            "import { ArchiveItemMutationDocument as B1 } from \"@example/web/graphql\";\nimport { ArchiveItemMutationDocument as B2 } from \"@example/checkout/graphql\";\nconst a = B1; const b = B2;",
            duplicate_name_config(),
            "/repo/apps/web/app/thing.ts",
        );

        // Local project by relative path, the other through its alias — the same
        // name resolving two different ways in one module.
        assert!(output.contains("./graphql/web.codegen"), "got:\n{output}");
        assert!(
            output.contains("@example/checkout/graphql/checkout.codegen"),
            "got:\n{output}"
        );
    }

    /// The transform as a real build runs it: SWC resolves the module before the
    /// plugin sees it and runs hygiene afterwards. Rewriting imports without
    /// keeping references and binding in one `SyntaxContext` only shows up here —
    /// hygiene is what turns the mismatch into an unbound identifier.
    fn transform_in_pipeline(
        source: &str,
        config: Config,
        filename: &str,
    ) -> (Module, Arc<SourceMap>) {
        use swc_core::common::{GLOBALS, Globals, Mark};
        use swc_core::ecma::transforms::base::{hygiene::hygiene, resolver};

        let cm = Arc::<SourceMap>::default();
        let fm = cm.new_source_file(FileName::Custom(filename.into()).into(), source.to_string());

        let mut parser = Parser::new(
            Syntax::Typescript(TsSyntax::default()),
            StringInput::from(&*fm),
            None,
        );
        let mut module = parser.parse_module().expect("Failed to parse module");

        GLOBALS.set(&Globals::default(), || {
            let unresolved_mark = Mark::new();
            let top_level_mark = Mark::new();
            module.visit_mut_with(&mut resolver(unresolved_mark, top_level_mark, true));
            module.visit_mut_with(&mut TransformVisitor::new(
                &config,
                Some(filename.to_string()),
            ));
            module.visit_mut_with(&mut hygiene());
        });

        (module, cm)
    }

    fn emit(module: &Module, cm: &Arc<SourceMap>) -> String {
        let mut buf = vec![];
        {
            let mut emitter = Emitter {
                cfg: Default::default(),
                cm: cm.clone(),
                comments: None,
                wr: Box::new(JsWriter::new(cm.clone(), "\n", &mut buf, None)),
            };
            emitter.emit_module(module).unwrap();
        }
        String::from_utf8(buf).unwrap()
    }

    /// Every identifier the module references in value position must be bound by
    /// something the module declares or imports. A rewritten import that lost its
    /// references leaves them behind as free variables — legal JavaScript that
    /// throws `ReferenceError` the moment it evaluates, which no type check,
    /// bundler, or bundle diff can see.
    fn assert_no_free_identifiers(module: &Module) {
        let declared: std::collections::HashSet<String> =
            swc_core::ecma::utils::collect_decls::<Id, _>(module)
                .into_iter()
                .map(|id| id.0.to_string())
                .collect();

        struct RefCollector(Vec<String>);
        impl Visit for RefCollector {
            fn visit_expr(&mut self, n: &Expr) {
                if let Expr::Ident(ident) = n {
                    self.0.push(ident.sym.to_string());
                }
                n.visit_children_with(self);
            }
        }

        let mut refs = RefCollector(vec![]);
        module.visit_with(&mut refs);

        let free: Vec<&String> = refs
            .0
            .iter()
            .filter(|name| !declared.contains(*name) && !GLOBAL_ALLOWLIST.contains(&name.as_str()))
            .collect();

        assert!(
            free.is_empty(),
            "unbound identifiers in the output: {:?}\ndeclared: {:?}",
            free,
            declared
        );
    }

    const GLOBAL_ALLOWLIST: &[&str] = &["Promise", "undefined"];

    #[test]
    fn test_cross_project_import_leaves_no_unbound_references() {
        // The reported failure: a generated file pulls documents from another
        // project's barrel and dereferences them at module scope. The import gets
        // redirected at the concrete codegen file, and every reference has to
        // follow it there.
        let (module, cm) = transform_in_pipeline(
            "import { ProductCardFragmentDoc, PriceFragmentDoc } from \"@example/catalog/graphql\";\nexport const D = { kind: \"Document\", definitions: [ProductCardFragmentDoc.definitions[0], PriceFragmentDoc.definitions[0]] };",
            two_document_cross_project_config(),
            "/repo/packages/storefront/graphql/components/thing.codegen.ts",
        );

        assert_no_free_identifiers(&module);

        let output = emit(&module, &cm);
        assert!(
            output.contains("import { ProductCardFragmentDoc }")
                && output.contains("import { PriceFragmentDoc }"),
            "both documents should keep their own name, got:\n{output}"
        );
    }

    #[test]
    fn test_same_project_document_leaves_no_unbound_references() {
        let (module, _cm) = transform_in_pipeline(
            "import { graphql } from \"./graphql\";\nexport const D = graphql(`fragment ProductCard on Product { id }`);",
            two_document_cross_project_config(),
            "/repo/packages/catalog/graphql/thing.ts",
        );

        assert_no_free_identifiers(&module);
    }

    #[test]
    fn test_aliased_cross_project_import_leaves_no_unbound_references() {
        // The same path with an alias already in the source: references are on the
        // alias, and the emitted import has to bind that name.
        let (module, cm) = transform_in_pipeline(
            "import { ProductCardFragmentDoc as Card } from \"@example/catalog/graphql\";\nexport const D = Card.definitions[0];",
            two_document_cross_project_config(),
            "/repo/packages/storefront/graphql/thing.codegen.ts",
        );

        assert_no_free_identifiers(&module);
        let output = emit(&module, &cm);
        assert!(
            output.contains("import { ProductCardFragmentDoc as Card }"),
            "got:\n{output}"
        );
    }

    /// Project A owns two documents in one codegen file; B imports both.
    fn two_document_cross_project_config() -> Config {
        let mut config = two_project_config();
        config.outputs.as_mut().unwrap()[0].manifest_data = Some(vec![
            ManifestEntry {
                source: "fragment ProductCard on Product { id }".to_string(),
                path: "./catalog.codegen".to_string(),
                name: "ProductCardFragmentDoc".to_string(),
            },
            ManifestEntry {
                source: "fragment Price on Product { price }".to_string(),
                path: "./catalog.codegen".to_string(),
                name: "PriceFragmentDoc".to_string(),
            },
        ]);
        config
    }

    #[test]
    fn test_same_document_name_from_two_outputs_keeps_two_bindings() {
        // Two projects export a document under the same name. One side is aliased
        // in the source, the other is not — they are still two documents and must
        // stay two imports, each pointing at its own project's codegen file.
        let (module, cm) = transform_in_pipeline(
            "import { ArchiveItemMutationDocument as B1 } from \"@example/web/graphql\";\nimport { ArchiveItemMutationDocument } from \"@example/checkout/graphql\";\nexport const a = B1.definitions[0];\nexport const b = ArchiveItemMutationDocument.definitions[0];",
            duplicate_name_config(),
            "/repo/apps/other/thing.ts",
        );

        assert_no_free_identifiers(&module);
        let output = emit(&module, &cm);

        assert!(
            output.contains(
                "import { ArchiveItemMutationDocument as B1 } from \"@example/web/graphql/web.codegen\""
            ),
            "the aliased side must keep pointing at web, got:\n{output}"
        );
        assert!(
            output.contains(
                "import { ArchiveItemMutationDocument } from \"@example/checkout/graphql/checkout.codegen\""
            ),
            "the unaliased side must keep its own binding, got:\n{output}"
        );
        assert!(
            output.contains("export const a = B1.definitions[0]")
                && output.contains("export const b = ArchiveItemMutationDocument.definitions[0]"),
            "neither reference may be redirected at the other document, got:\n{output}"
        );
    }

    #[test]
    fn test_same_document_name_from_import_and_graphql_call_keeps_two_bindings() {
        // The same collision without an alias to separate them: one project's
        // document arrives as a named import, the other's through a graphql() call
        // in the same module. The second one has to be given a free name.
        let (module, cm) = transform_in_pipeline(
            "import { graphql } from \"@example/web/graphql\";\nimport { ArchiveItemMutationDocument } from \"@example/checkout/graphql\";\nexport const a = ArchiveItemMutationDocument.definitions[0];\nexport const b = graphql(`mutation ArchiveItem { archiveItem { id } }`);",
            duplicate_name_config(),
            "/repo/apps/other/thing.ts",
        );

        assert_no_free_identifiers(&module);
        let output = emit(&module, &cm);

        assert!(
            output.contains(
                "import { ArchiveItemMutationDocument } from \"@example/checkout/graphql/checkout.codegen\""
            ),
            "the named import keeps the name it was written with, got:\n{output}"
        );
        assert!(
            output.contains("@example/web/graphql/web.codegen"),
            "web's document must still be imported, got:\n{output}"
        );
        assert!(
            output.contains("export const a = ArchiveItemMutationDocument.definitions[0]"),
            "the named import's reference must not move, got:\n{output}"
        );
        assert!(
            !output.contains("export const b = ArchiveItemMutationDocument;"),
            "the graphql() call must not be bound to checkout's document, got:\n{output}"
        );
    }

    // --- re-exports of documents ----------------------------------------------

    /// Two documents in different generated files, so a single declaration naming
    /// both has to split.
    fn reexport_config() -> Config {
        Config {
            manifest_data: Some(vec![
                ManifestEntry {
                    source: "query { me { id } }".to_string(),
                    path: "./query.codegen".to_string(),
                    name: "MyQueryDocument".to_string(),
                },
                ManifestEntry {
                    source: "fragment F on User { id }".to_string(),
                    path: "./other.codegen".to_string(),
                    name: "FFragmentDoc".to_string(),
                },
            ]),
            output_dir: ".".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_document_reexport_points_at_the_generated_file() {
        // The entrypoint is emptied in its own compilation, so a re-export left
        // pointing at it resolves to nothing — a barrel that silently exports
        // undefined, which no type check or bundler treats as an error.
        let output = transform(
            "export { MyQueryDocument } from './graphql';",
            reexport_config(),
            "test.ts",
        );

        assert!(
            output.contains("export { MyQueryDocument } from \"./query.codegen\""),
            "got:\n{output}"
        );
        assert!(!output.contains("./graphql"), "got:\n{output}");
    }

    #[test]
    fn test_document_reexport_splits_by_generated_file() {
        let output = transform(
            "export { MyQueryDocument as Q, FFragmentDoc } from './graphql';",
            reexport_config(),
            "test.ts",
        );

        assert!(
            output.contains("export { MyQueryDocument as Q } from \"./query.codegen\""),
            "the exported name must survive the move, got:\n{output}"
        );
        assert!(
            output.contains("export { FFragmentDoc } from \"./other.codegen\""),
            "got:\n{output}"
        );
    }

    #[test]
    fn test_type_only_reexport_is_dropped() {
        // Erased before this output runs, and the entrypoint it named is emptied,
        // so there is nothing left to carry — as with a type-only import.
        let output = transform(
            "export type { MyQueryDocument } from './graphql';\nexport const x = 1;",
            reexport_config(),
            "test.ts",
        );

        assert!(!output.contains("./graphql"), "got:\n{output}");
        assert!(output.contains("export const x = 1"), "got:\n{output}");
    }

    #[test]
    #[should_panic(expected = "star re-export")]
    fn test_star_reexport_of_an_entrypoint_is_an_error() {
        transform("export * from './graphql';", reexport_config(), "test.ts");
    }

    #[test]
    #[should_panic(expected = "star re-export")]
    fn test_namespace_reexport_of_an_entrypoint_is_an_error() {
        transform(
            "export * as all from './graphql';",
            reexport_config(),
            "test.ts",
        );
    }

    #[test]
    #[should_panic(expected = "does not exist at runtime")]
    fn test_reexporting_the_tag_from_an_entrypoint_is_an_error() {
        transform(
            "export { graphql } from './graphql';",
            reexport_config(),
            "test.ts",
        );
    }

    #[test]
    #[should_panic(expected = "fully inlined")]
    fn test_locally_reexporting_the_tag_is_an_error() {
        // Not an expression, so the usage validator never saw it, and the module
        // came out exporting a name nothing declared.
        transform(
            "import { graphql } from './graphql'; const q = graphql(`query { me { id } }`); export { graphql };",
            reexport_config(),
            "test.ts",
        );
    }

    #[test]
    fn test_new_imports_keep_the_position_of_the_ones_they_replace() {
        // Hoisting them to the top puts the generated file's module-init work
        // ahead of a side-effect import that was written to run first.
        let output = transform(
            "import './polyfill';\nimport { graphql } from './graphql';\nconst q = graphql(`query { me { id } }`);",
            reexport_config(),
            "test.ts",
        );

        let polyfill = output.find("./polyfill").expect("polyfill import kept");
        let codegen = output
            .find("./query.codegen")
            .expect("codegen import added");
        assert!(polyfill < codegen, "got:\n{output}");
    }

    // --- renames land on bindings, not on names -------------------------------

    #[test]
    fn test_rename_keeps_the_exported_name_and_property_key() {
        // An earlier aliased import of the same document makes the second one
        // resolve to that alias. `export { X }` and `{ X }` both mean `X as X`, so
        // renaming in place would move the module's public export name and an
        // object key along with the binding.
        let (module, cm) = transform_in_pipeline(
            "import { MyQueryDocument as Doc } from './graphql';\nimport { MyQueryDocument } from './graphql';\nexport { MyQueryDocument };\nexport const o = { MyQueryDocument };\nexport const use1 = Doc;",
            reexport_config(),
            "test.ts",
        );

        assert_no_free_identifiers(&module);
        let output = emit(&module, &cm);

        assert!(
            output.contains("export { Doc as MyQueryDocument }"),
            "the exported name must not move, got:\n{output}"
        );
        assert!(
            output.contains("MyQueryDocument: Doc"),
            "the property key must not move, got:\n{output}"
        );
    }

    #[test]
    fn test_identity_rename_leaves_shorthand_alone() {
        let (module, cm) = transform_in_pipeline(
            "import { MyQueryDocument } from './graphql';\nexport { MyQueryDocument };\nexport const o = { MyQueryDocument };",
            reexport_config(),
            "test.ts",
        );

        assert_no_free_identifiers(&module);
        let output = emit(&module, &cm);

        assert!(
            output.contains("export { MyQueryDocument };"),
            "got:\n{output}"
        );
        assert!(
            !output.contains("MyQueryDocument: MyQueryDocument"),
            "no reason to expand it, got:\n{output}"
        );
    }
}
