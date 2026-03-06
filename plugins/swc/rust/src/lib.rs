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

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    #[serde(rename = "manifestPath")]
    pub manifest_path: Option<String>,
    #[serde(rename = "manifestData")]
    pub manifest_data: Option<Vec<ManifestEntry>>,
    /// Absolute path to the output directory where codegen files are located
    #[serde(rename = "outputDir")]
    pub output_dir: String,
    #[serde(rename = "graphqlImportPaths")]
    pub graphql_import_paths: Option<Vec<String>>,
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

pub struct TransformVisitor {
    manifest: HashMap<String, ManifestEntry>,
    name_to_entry: HashMap<String, ManifestEntry>,
    output_dir: PathBuf,
    current_file: Option<PathBuf>,
    new_imports: HashMap<String, (String, String)>, // local_name -> (imported_name, source_path)
    graphql_ids: std::collections::HashSet<Id>,
    graphql_import_paths: Vec<String>,
    emit_extensions: EmitExtensions,
    existing_names: std::collections::HashSet<String>,
    document_name_to_local_name: HashMap<String, String>,
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

impl TransformVisitor {
    pub fn new(config: &Config, current_file: Option<String>) -> Self {
        let entries = if let Some(data) = &config.manifest_data {
            data.clone()
        } else if let Some(path) = &config.manifest_path {
            let manifest_content =
                std::fs::read_to_string(path).unwrap_or_else(|_| "[]".to_string());
            serde_json::from_str(&manifest_content).unwrap_or_default()
        } else {
            vec![]
        };

        let mut manifest = HashMap::new();
        let mut name_to_entry = HashMap::new();
        for entry in entries {
            manifest.insert(normalize(&entry.source), entry.clone());
            name_to_entry.insert(entry.name.clone(), entry);
        }

        Self {
            manifest,
            name_to_entry,
            output_dir: PathBuf::from(&config.output_dir),
            current_file: current_file.map(PathBuf::from),
            new_imports: HashMap::new(),
            graphql_ids: std::collections::HashSet::new(),
            graphql_import_paths: config.graphql_import_paths.clone().unwrap_or_default(),
            emit_extensions: config.emit_extensions.clone(),
            existing_names: std::collections::HashSet::new(),
            document_name_to_local_name: HashMap::new(),
            id_renames: HashMap::new(),
        }
    }

    fn get_local_name(&mut self, document_name: &str) -> String {
        if let Some(local_name) = self.document_name_to_local_name.get(document_name) {
            return local_name.clone();
        }

        let mut unique_name = document_name.to_string();
        if self.existing_names.contains(&unique_name) {
            let mut i = 1;
            while self
                .existing_names
                .contains(&format!("{}{}", document_name, i))
            {
                i += 1;
            }
            unique_name = format!("{}{}", document_name, i);
        }

        self.document_name_to_local_name
            .insert(document_name.to_string(), unique_name.clone());
        unique_name
    }

    fn get_relative_import_path(&self, codegen_rel_path: &str) -> String {
        let mut result = if let Some(current_file) = &self.current_file {
            let codegen_abs_path = self.output_dir.join(codegen_rel_path);
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

    fn is_our_graphql_path(&self, src: &str) -> bool {
        // Normalize incoming src path for comparison on Windows
        let src = if cfg!(windows) {
            src.replace('\\', "/")
        } else {
            src.to_string()
        };
        let src = src.as_str();
        let src_no_ext = strip_script_extension(src);

        for path in &self.graphql_import_paths {
            if src == path || src_no_ext == strip_script_extension(path) {
                return true;
            }
        }

        if let Some(current_file) = &self.current_file {
            let entrypoint_abs_path = self.output_dir.join("graphql");
            let index_abs_path = self.output_dir.join("index");
            let entrypoint_abs_no_ext = strip_script_extension_path(&entrypoint_abs_path);
            let index_abs_no_ext = strip_script_extension_path(&index_abs_path);

            if let Some(parent) = current_file.parent() {
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
                        return true;
                    }
                }
            }

            if let Some(parent) = current_file.parent()
                && let Some(rel_path) = pathdiff::diff_paths(&entrypoint_abs_path, parent)
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
                    return true;
                }

                // Handle directory import resolving to index
                if format!("{}/index", src_normalized) == our_index_normalized {
                    return true;
                }
            }
        }

        false
    }
}

impl VisitMut for TransformVisitor {
    fn visit_mut_module(&mut self, n: &mut Module) {
        // If we are processing the entrypoint itself, clear it
        if let Some(current_file) = &self.current_file {
            let entrypoint_abs_path = self.output_dir.join("graphql");
            let current_file_normalized = current_file.with_extension("");
            if current_file_normalized == entrypoint_abs_path {
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
                                init: Some(Box::new(Expr::Arrow(ArrowExpr {
                                    span: DUMMY_SP,
                                    ctxt: SyntaxContext::empty(),
                                    params: vec![],
                                    body: Box::new(BlockStmtOrExpr::Expr(Box::new(Expr::Lit(
                                        Lit::Null(Null { span: DUMMY_SP }),
                                    )))),
                                    is_async: false,
                                    is_generator: false,
                                    type_params: None,
                                    return_type: None,
                                }))),
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
                if self.is_our_graphql_path(src) {
                    let import_is_type_only = import.type_only;
                    for specifier in &import.specifiers {
                        if let ImportSpecifier::Named(named) = specifier {
                            let local_name = named.local.sym.as_str();
                            let imported_name = named
                                .imported
                                .as_ref()
                                .map(|i| match i {
                                    ModuleExportName::Ident(id) => id.sym.as_str(),
                                    ModuleExportName::Str(s) => s.value.as_str().unwrap_or(""),
                                })
                                .unwrap_or(local_name);

                            let specifier_is_type_only = import_is_type_only || named.is_type_only;

                            our_import_ids.insert(named.local.to_id());

                            if imported_name == "graphql" || imported_name == "gql" {
                                self.graphql_ids.insert(named.local.to_id());
                            } else if self.name_to_entry.contains_key(imported_name)
                                && !specifier_is_type_only
                            {
                                // Only track non-type-only imports of document names
                                let entry = self.name_to_entry.get(imported_name).unwrap();
                                let rel_path = self.get_relative_import_path(&entry.path);

                                let target_local_name = if local_name != imported_name {
                                    // Aliased, keep it and register it
                                    self.document_name_to_local_name
                                        .insert(imported_name.to_string(), local_name.to_string());
                                    local_name.to_string()
                                } else {
                                    self.get_local_name(imported_name)
                                };

                                self.new_imports.insert(
                                    target_local_name.clone(),
                                    (imported_name.to_string(), rel_path),
                                );

                                if target_local_name != local_name {
                                    self.id_renames
                                        .insert(named.local.to_id(), target_local_name);
                                }
                            }
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

        // Remove ALL imports from graphql.ts/index.ts (it's cleared to stubs)
        n.body.retain_mut(|item| {
            if let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item {
                let src = import.src.value.as_str().unwrap_or("");
                if self.is_our_graphql_path(src) {
                    return false;
                }
            }
            true
        });

        // Add new imports at the top (sorted alphabetically by local name)
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
                i,
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
        }
    }

    fn visit_mut_expr(&mut self, n: &mut Expr) {
        n.visit_mut_children_with(self);

        if let Expr::Call(call) = n
            && let Callee::Expr(callee_expr) = &call.callee
            && let Expr::Ident(ident) = &**callee_expr
            && self.graphql_ids.contains(&ident.to_id())
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

            if let Some(source) = source
                && let Some(entry) = self.manifest.get(&source)
            {
                let entry_name = entry.name.clone();
                let entry_path = entry.path.clone();
                let rel_path = self.get_relative_import_path(&entry_path);
                let target_local_name = self.get_local_name(&entry_name);
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
}

#[plugin_transform]
pub fn process_transform(
    mut program: Program,
    _metadata: TransformPluginProgramMetadata,
) -> Program {
    let config_str = _metadata.get_transform_plugin_config();
    let config: Config = serde_json::from_str(&config_str.unwrap_or_else(|| "{}".to_string()))
        .unwrap_or_else(|_| Config {
            manifest_path: None,
            manifest_data: None,
            output_dir: "".to_string(),
            graphql_import_paths: None,
            emit_extensions: EmitExtensions::None,
        });

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
    fn test_visitor_mixed_imports() {
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
        };

        let output = transform(
            "import { graphql, other } from './graphql'; const q = graphql(`query { me { id } }`);",
            config,
            "test.ts",
        );

        assert!(output.contains("import { MyQueryDocument } from \"./query.codegen\";"));
        assert!(!output.contains("other"));
        assert!(!output.contains("from './graphql'"));
        assert!(output.contains("const q = MyQueryDocument;"));
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
        };

        let output = transform(
            "export const graphql = () => { /* big map */ }; export const gql = graphql;",
            config,
            "/root/gen/graphql.ts",
        );

        assert!(output.contains("export const graphql = ()=>null"));
        assert!(output.contains("export const gql = graphql"));
        assert!(!output.contains("big map"));
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
        };

        let output = transform(
            "import { GetUserQueryDocument, OtherType } from '../gen/graphql';",
            config,
            "/root/app/test.ts",
        );

        assert!(output.contains("import { GetUserQueryDocument } from \"../gen/query.codegen\";"));
        assert!(!output.contains("OtherType"));
        assert!(!output.contains("from '../gen/graphql'"));
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
}
