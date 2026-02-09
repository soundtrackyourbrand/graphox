use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use swc_core::common::{DUMMY_SP, SyntaxContext};
use swc_core::ecma::ast::*;
use swc_core::ecma::visit::{VisitMut, VisitMutWith};
use swc_core::plugin::{metadata::TransformPluginProgramMetadata, plugin_transform};

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
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ManifestEntry {
    pub source: String,
    pub path: String,
    pub name: String,
}

pub struct TransformVisitor {
    manifest: HashMap<String, ManifestEntry>,
    output_dir: PathBuf,
    current_file: Option<PathBuf>,
    new_imports: HashMap<String, String>, // local_name -> source_path
    graphql_ids: std::collections::HashSet<Id>,
    graphql_import_paths: Vec<String>,
}

fn normalize(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace()).collect()
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
        for entry in entries {
            manifest.insert(normalize(&entry.source), entry);
        }

        Self {
            manifest,
            output_dir: PathBuf::from(&config.output_dir),
            current_file: current_file.map(PathBuf::from),
            new_imports: HashMap::new(),
            graphql_ids: std::collections::HashSet::new(),
            graphql_import_paths: config.graphql_import_paths.clone().unwrap_or_default(),
        }
    }

    fn get_relative_import_path(&self, codegen_rel_path: &str) -> String {
        if let Some(current_file) = &self.current_file {
            let codegen_abs_path = self.output_dir.join(codegen_rel_path);
            if let Some(parent) = current_file.parent()
                && let Some(rel_path) = pathdiff::diff_paths(&codegen_abs_path, parent)
            {
                let mut s = rel_path.to_string_lossy().to_string();
                if !s.starts_with('.') && !s.starts_with('/') {
                    s = format!("./{}", s);
                }
                return s;
            }
        }
        // Fallback to the path in the manifest
        codegen_rel_path.to_string()
    }

    fn is_our_graphql_path(&self, src: &str) -> bool {
        // 1. Check explicit config paths
        for path in &self.graphql_import_paths {
            if src == path
                || src.strip_suffix(".js") == Some(path)
                || src.strip_suffix(".ts") == Some(path)
                || path.strip_suffix(".js") == Some(src)
                || path.strip_suffix(".ts") == Some(src)
            {
                return true;
            }
        }

        // 2. Check for subpath imports starting with # if they contain 'graphql'
        if src.starts_with('#') && src.contains("graphql") {
            return true;
        }

        // 3. Fallback to relative path detection
        if let Some(current_file) = &self.current_file {
            let entrypoint_abs_path = self.output_dir.join("graphql");
            if let Some(parent) = current_file.parent()
                && let Some(rel_path) = pathdiff::diff_paths(&entrypoint_abs_path, parent)
            {
                let mut s = rel_path.to_string_lossy().to_string();
                if !s.starts_with('.') && !s.starts_with('/') {
                    s = format!("./{}", s);
                }

                let src_normalized = src
                    .strip_suffix(".js")
                    .unwrap_or(src)
                    .strip_suffix(".ts")
                    .unwrap_or(src);
                let our_normalized = s
                    .strip_suffix(".js")
                    .unwrap_or(&s)
                    .strip_suffix(".ts")
                    .unwrap_or(&s);

                return src_normalized == our_normalized;
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

        // First pass: identify imports from our graphql.ts
        for item in &n.body {
            if let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item {
                let src = import.src.value.as_str().unwrap_or("");
                if self.is_our_graphql_path(src) {
                    for specifier in &import.specifiers {
                        if let ImportSpecifier::Named(named) = specifier
                            && (named.local.sym == "graphql" || named.local.sym == "gql")
                        {
                            self.graphql_ids.insert(named.local.to_id());
                        }
                    }
                }
            }
        }

        n.visit_mut_children_with(self);

        n.body.retain_mut(|item| {
            if let ModuleItem::ModuleDecl(ModuleDecl::Import(import)) = item {
                let src = import.src.value.as_str().unwrap_or("");
                if self.is_our_graphql_path(src) {
                    import.specifiers.retain(|specifier| match specifier {
                        ImportSpecifier::Named(named) => {
                            !self.graphql_ids.contains(&named.local.to_id())
                        }
                        _ => true,
                    });
                    return !import.specifiers.is_empty();
                }
            }
            true
        });

        let mut imports: Vec<_> = self.new_imports.iter().collect();
        imports.sort_by(|a, b| a.0.cmp(b.0));

        for (i, (local_name, source_path)) in imports.into_iter().enumerate() {
            n.body.insert(
                i,
                ModuleItem::ModuleDecl(ModuleDecl::Import(ImportDecl {
                    span: DUMMY_SP,
                    specifiers: vec![ImportSpecifier::Named(ImportNamedSpecifier {
                        span: DUMMY_SP,
                        local: Ident::new(local_name.clone().into(), DUMMY_SP, Default::default()),
                        imported: None,
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
                _ => None,
            };

            if let Some(source) = source
                && let Some(entry) = self.manifest.get(&source)
            {
                let rel_path = self.get_relative_import_path(&entry.path);
                self.new_imports.insert(entry.name.clone(), rel_path);
                *n = Expr::Ident(Ident::new(
                    entry.name.clone().into(),
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
        };

        let output = transform(
            "import { graphql } from './graphql'; const q = graphql(`query { me { id } }`);",
            config,
            "test.ts",
        );

        assert!(output.contains("import { MyQueryDocument } from \"./query.codegen\";"));
        assert!(output.contains("const q = MyQueryDocument;"));
        assert!(!output.contains("import { graphql }"));
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
        };

        let output = transform(
            "import { graphql, other } from './graphql'; const q = graphql(`query { me { id } }`);",
            config,
            "test.ts",
        );

        assert!(output.contains("import { MyQueryDocument } from \"./query.codegen\";"));
        assert!(output.contains("import { other } from './graphql';"));
        assert!(output.contains("const q = MyQueryDocument;"));
        assert!(!output.contains("graphql,"));
        assert!(!output.contains(" graphql "));
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
            graphql_import_paths: None,
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
}
