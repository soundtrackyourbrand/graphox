use std::fs;

use crate::support::builders::FragmentInfoBuilder;
use crate::support::completion_items_array;
use crate::support::create_doc;
use crate::support::lsp::LspTestScenario;
use crate::support::lsp_did_open;
use crate::support::lsp_request_completion;
use crate::support::make_temp_project_with_schema;
use crate::support::pos;
use crate::support::with_cursor;
use crate::support::write_project_file;
use apollo_compiler::Schema;
use graphox::features::diagnostics::DocumentDiagnostics;
use graphox_core::Config;
use graphox_core::config::GlobPattern;
use graphox_core::config::ProjectConfig;
use graphox_core::config::SchemaSource;
use tempfile::TempDir;
use tower_lsp::lsp_types::DiagnosticSeverity;
use tower_lsp::lsp_types::Url;

#[test]
#[ntest::timeout(100)]
fn test_private_shadows_public_warning() {
    let scenario = LspTestScenario::new()
        .with_file("package.json", "{}")
        .with_file("pkg_a/pkg.json", "{}")
        .with_file("pkg_b/pkg.json", "{}")
        .with_file("pkg_a/frag.graphql", "fragment ShadowFrag on User { id }")
        .with_file(
            "pkg_b/frag.graphql",
            "fragment ShadowFrag on User @public { name }",
        );

    let base = scenario.write_files().unwrap();
    let pkg_a = base.join("pkg_a");
    let frag_a_path = pkg_a.join("frag.graphql");
    let text_a = "fragment ShadowFrag on User { id }";

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let doc = create_doc(uri_a.as_str(), text_a);

    let schema = Schema::parse(
        "type User { id: ID! name: String } type Query { me: User }",
        "schema.graphql",
    )
    .unwrap()
    .validate()
    .unwrap();

    let public_frag = FragmentInfoBuilder::new("ShadowFrag", "User")
        .public()
        .with_uri(Url::from_file_path(base.join("pkg_b").join("frag.graphql")).unwrap())
        .with_package_root(base.join("pkg_b"))
        .build();

    let diagnostics =
        doc.get_semantic_diagnostics(&schema, &[public_frag], None, None, false, true);

    let shadowing_diags: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("shadows"))
        .collect();
    assert_eq!(
        shadowing_diags.len(),
        1,
        "Should have exactly one shadowing warning"
    );
    assert_eq!(shadowing_diags[0].severity, Some(DiagnosticSeverity::HINT));
}

#[test]
#[ntest::timeout(100)]
fn test_public_collision_error() {
    let scenario = LspTestScenario::new()
        .with_file("package.json", "{}")
        .with_file(
            "pkg_a/frag.graphql",
            "fragment CollideFrag on User @public { id }",
        )
        .with_file(
            "pkg_b/frag.graphql",
            "fragment CollideFrag on User @public { name }",
        );

    let base = scenario.write_files().unwrap();
    let frag_a_path = base.join("pkg_a").join("frag.graphql");
    let text_a = "fragment CollideFrag on User @public { id }";

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let doc = create_doc(uri_a.as_str(), text_a);

    let schema = Schema::parse(
        "type User { id: ID! name: String } type Query { me: User }",
        "schema.graphql",
    )
    .unwrap()
    .validate()
    .unwrap();

    let other_frag = FragmentInfoBuilder::new("CollideFrag", "User")
        .public()
        .with_uri(Url::from_file_path(base.join("pkg_b").join("frag.graphql")).unwrap())
        .with_package_root(base.join("pkg_b"))
        .build();

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[other_frag], None, None, false, true);

    let collision_diags: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("Duplicate public fragment"))
        .collect();
    assert_eq!(
        collision_diags.len(),
        1,
        "Should have exactly one collision error for public fragments"
    );
    assert_eq!(collision_diags[0].severity, Some(DiagnosticSeverity::ERROR));
}

#[test]
#[ntest::timeout(100)]
fn test_cross_project_fragment_usage() {
    let scenario = LspTestScenario::new()
        .with_file("package.json", "{}")
        .with_file(
            "pkg_a/frag.graphql",
            "fragment PublicFrag on User @public { id }",
        )
        .with_file("query.graphql", "query { me { ...PublicFrag } }");

    let base = scenario.write_files().unwrap();
    let frag_a_path = base.join("pkg_a").join("frag.graphql");
    let query_path = base.join("query.graphql");

    let uri_frag = Url::from_file_path(&frag_a_path).unwrap();
    let uri_query = Url::from_file_path(&query_path).unwrap();

    let schema = Schema::parse(
        "type User { id: ID! } type Query { me: User }",
        "schema.graphql",
    )
    .unwrap()
    .validate()
    .unwrap();

    let public_frag = FragmentInfoBuilder::new("PublicFrag", "User")
        .public()
        .with_uri(uri_frag.clone())
        .with_package_root(base.join("pkg_a"))
        .build();

    let doc = create_doc(uri_query.as_str(), "query { me { ...PublicFrag } }");

    let diagnostics =
        doc.get_semantic_diagnostics(&schema, &[public_frag], None, None, false, true);

    let undefined_diags: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("Undefined fragment"))
        .collect();
    assert!(
        undefined_diags.is_empty(),
        "Public fragment from other project should be available: {:?}",
        diagnostics
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_private_fragment_not_in_completion_other_package() {
    let schema = "type User { id: ID! name: String! } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    let frag_text = "fragment PrivateFrag on User { id }";
    let query_text = "query { user { ... } }";
    let frag_uri = write_project_file(&dir, "pkg_a/frag.graphql", frag_text);
    let query_uri = write_project_file(&dir, "pkg_b/query.graphql", query_text);

    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let completions = lsp_request_completion(&mut service, query_uri.clone(), pos(1, 17)).await;
    let items = completion_items_array(&completions);

    let has_private = items.iter().any(|i| i.label == "PrivateFrag");
    assert!(
        !has_private,
        "Private fragment from other package should NOT appear in completions"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_public_fragment_in_completion_other_package() {
    let schema = "type User { id: ID! name: String! } type Query { user: User }";
    let dir = TempDir::new().expect("failed to create tempdir");
    let schema_path = dir.path().join("schema.graphql");
    fs::write(&schema_path, schema).expect("write schema");
    fs::write(dir.path().join("package.json"), "{}").expect("write package.json");

    let config = Config::new_test(
        dir.path().to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("pkg_a/**/*.graphql".to_string())),
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("pkg_b/**/*.graphql".to_string())),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    let frag_text = "fragment PublicFrag on User @public { id }";
    let (query_text, cursor_pos) = with_cursor("query { user { ...| } }");
    let frag_uri = write_project_file(&dir, "pkg_a/frag.graphql", frag_text);
    let query_uri = write_project_file(&dir, "pkg_b/query.graphql", &query_text);

    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let completions = lsp_request_completion(&mut service, query_uri.clone(), cursor_pos).await;
    let items = completion_items_array(&completions);

    let has_public = items.iter().any(|i| i.label == "PublicFrag");
    assert!(
        has_public,
        "Public fragment from other package should appear in completions"
    );
}
