use crate::support::assert_diag_range_equals;
use crate::support::assert_diagnostic_with_message;
use crate::support::assert_diagnostics_count;
use crate::support::builders::FragmentInfoBuilder;
use crate::support::create_doc;
use apollo_compiler::Schema;
use graphox::features::diagnostics::DocumentDiagnostics;
use tower_lsp::lsp_types::*;

#[test]
#[ntest::timeout(100)]
fn test_private_duplicate_same_package_root_reports_error() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file("package.json", "{}")
        .with_file("pkg/a.graphql", "fragment DuplicateFrag on User { id }")
        .with_file("pkg/b.graphql", "fragment DuplicateFrag on User { name }");

    let base = scenario.write_files().unwrap();
    let pkg = base.join("pkg");
    let frag_a_path = pkg.join("a.graphql");
    let frag_b_path = pkg.join("b.graphql");
    let text_a = "fragment DuplicateFrag on User { id }";

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let doc = create_doc(uri_a.as_str(), text_a);

    let schema = Schema::parse(
        "type User { id: ID! name: String } type Query { me: User }",
        "schema.graphql",
    )
    .unwrap()
    .validate()
    .unwrap();

    let other_frag = FragmentInfoBuilder::new("DuplicateFrag", "User")
        .with_uri(Url::from_file_path(&frag_b_path).unwrap())
        .with_package_root(base.clone())
        .build();

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[other_frag], None, None, false, true);

    assert_diagnostics_count(&diagnostics, 1);
    let diag = assert_diagnostic_with_message(&diagnostics, "Duplicate fragment name");
    assert_diag_range_equals(
        diag,
        &crate::support::range_for_token(&doc, text_a, "DuplicateFrag"),
    );
}

#[test]
#[ntest::timeout(100)]
fn test_private_duplicate_same_project_via_config_reports_error() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file("package.json", "{}")
        .with_file("pkg_a/a.graphql", "fragment DuplicateFrag on User { id }")
        .with_file("pkg_b/b.graphql", "fragment DuplicateFrag on User { name }");

    let base = scenario.write_files().unwrap();
    let pkg_a = base.join("pkg_a");
    let pkg_b = base.join("pkg_b");
    let frag_a_path = pkg_a.join("a.graphql");
    let frag_b_path = pkg_b.join("b.graphql");
    let text_a = "fragment DuplicateFrag on User { id }";

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let doc = create_doc(uri_a.as_str(), text_a);

    let schema = Schema::parse(
        "type User { id: ID! name: String } type Query { me: User }",
        "schema.graphql",
    )
    .unwrap()
    .validate()
    .unwrap();

    let config = graphox::Config {
        output_dir: None,
        projects: vec![graphox::config::ProjectConfig {
            schema: graphox::config::SchemaSource::Single("schema.graphql".to_string()),
            include: graphox::config::GlobPattern::Single("**/*.graphql".to_string()),
            codegen: Some(false),
            ..Default::default()
        }],
        base_dir: base.clone(),
        ..graphox::Config::new_empty()
    };

    let other_frag = FragmentInfoBuilder::new("DuplicateFrag", "User")
        .with_uri(Url::from_file_path(&frag_b_path).unwrap())
        .with_package_root(base.clone())
        .build();

    let diagnostics =
        doc.get_semantic_diagnostics(&schema, &[other_frag], None, Some(&config), false, true);

    assert_diagnostics_count(&diagnostics, 1);
    let diag = assert_diagnostic_with_message(&diagnostics, "Duplicate fragment name");
    assert_diag_range_equals(
        diag,
        &crate::support::range_for_token(&doc, text_a, "DuplicateFrag"),
    );
}

#[test]
#[ntest::timeout(100)]
fn test_public_duplicate_across_workspace_reports_error() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file("package.json", "{}")
        .with_file(
            "pkg_a/a.graphql",
            "fragment PublicFrag on User @public { id }",
        )
        .with_file(
            "pkg_b/b.graphql",
            "fragment PublicFrag on User @public { name }",
        );

    let base = scenario.write_files().unwrap();
    let pkg_a = base.join("pkg_a");
    let pkg_b = base.join("pkg_b");
    let frag_a_path = pkg_a.join("a.graphql");
    let frag_b_path = pkg_b.join("b.graphql");
    let text_a = "fragment PublicFrag on User @public { id }";

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let doc = create_doc(uri_a.as_str(), text_a);

    let schema = Schema::parse(
        "type User { id: ID! name: String } type Query { me: User }",
        "schema.graphql",
    )
    .unwrap()
    .validate()
    .unwrap();

    let other_frag = FragmentInfoBuilder::new("PublicFrag", "User")
        .public()
        .with_uri(Url::from_file_path(&frag_b_path).unwrap())
        .with_package_root(base.clone())
        .build();

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[other_frag], None, None, false, true);

    assert_diagnostics_count(&diagnostics, 1);
    let diag = assert_diagnostic_with_message(&diagnostics, "Duplicate public fragment name");
    assert_diag_range_equals(
        diag,
        &crate::support::range_for_token(&doc, text_a, "PublicFrag"),
    );
}

#[test]
#[ntest::timeout(100)]
fn test_private_shadows_public_emits_hint() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file("package.json", "{}")
        .with_file("pkg_a/a.graphql", "fragment Shadowed on User { id }")
        .with_file(
            "pkg_b/b.graphql",
            "fragment Shadowed on User @public { name }",
        );

    let base = scenario.write_files().unwrap();
    let pkg_a = base.join("pkg_a");
    let pkg_b = base.join("pkg_b");
    let frag_a_path = pkg_a.join("a.graphql");
    let frag_b_path = pkg_b.join("b.graphql");
    let text_a = "fragment Shadowed on User { id }";

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let doc = create_doc(uri_a.as_str(), text_a);

    let schema = Schema::parse(
        "type User { id: ID! name: String } type Query { me: User }",
        "schema.graphql",
    )
    .unwrap()
    .validate()
    .unwrap();

    let public_frag = FragmentInfoBuilder::new("Shadowed", "User")
        .public()
        .with_uri(Url::from_file_path(&frag_b_path).unwrap())
        .with_package_root(pkg_b.clone())
        .build();

    let diagnostics =
        doc.get_semantic_diagnostics(&schema, &[public_frag], None, None, false, true);

    assert_diagnostics_count(&diagnostics, 1);
    let diag = &diagnostics[0];
    assert!(diag.message.contains("shadows a public fragment"));
    assert_eq!(diag.severity, Some(DiagnosticSeverity::HINT));
    assert_diag_range_equals(
        diag,
        &crate::support::range_for_token(&doc, text_a, "Shadowed"),
    );
}

#[test]
#[ntest::timeout(100)]
fn test_private_duplicates_across_different_projects_do_not_error() {
    let scenario = crate::support::lsp::LspTestScenario::new()
        .with_file("pkg_a/package.json", "{}")
        .with_file("pkg_b/package.json", "{}")
        .with_file("pkg_a/a.graphql", "fragment DuplicateFrag on User { id }")
        .with_file("pkg_b/b.graphql", "fragment DuplicateFrag on User { name }");

    let base = scenario.write_files().unwrap();
    let pkg_a = base.join("pkg_a");
    let pkg_b = base.join("pkg_b");
    let frag_a_path = pkg_a.join("a.graphql");
    let frag_b_path = pkg_b.join("b.graphql");

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let doc = create_doc(
        uri_a.as_str(),
        &std::fs::read_to_string(&frag_a_path).unwrap(),
    );

    let schema = Schema::parse(
        "type User { id: ID! name: String } type Query { me: User }",
        "schema.graphql",
    )
    .unwrap()
    .validate()
    .unwrap();

    let other_frag = FragmentInfoBuilder::new("DuplicateFrag", "User")
        .with_uri(Url::from_file_path(&frag_b_path).unwrap())
        .with_package_root(pkg_b.clone())
        .build();

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[other_frag], None, None, false, true);

    assert!(
        diagnostics.is_empty(),
        "Did not expect duplicate fragment error across different projects, got: {:?}",
        diagnostics
    );
}
