use crate::support::create_doc;
use apollo_compiler::Schema;
use graphql_rust::features::completion::FragmentCompletionInfo;
use graphql_rust::features::diagnostics::DocumentDiagnostics;
use tempfile::tempdir;
use tower_lsp::lsp_types::*;

#[test]
#[ntest::timeout(100)]
fn test_private_duplicate_same_package_root_reports_error() {
    let dir = tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let pkg = base.join("pkg");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(base.join("package.json"), "{}").unwrap();

    let frag_a_path = pkg.join("a.graphql");
    let frag_b_path = pkg.join("b.graphql");

    let text_a = "fragment DuplicateFrag on User { id }";
    std::fs::write(&frag_a_path, text_a).unwrap();
    std::fs::write(&frag_b_path, "fragment DuplicateFrag on User { name }").unwrap();

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let doc = create_doc(uri_a.as_str(), text_a);

    let schema = Schema::parse(
        "type User { id: ID! name: String } type Query { me: User }",
        "schema.graphql",
    )
    .unwrap()
    .validate()
    .unwrap();

    let other_frag = FragmentCompletionInfo {
        name: "DuplicateFrag".to_string(),
        type_condition: "User".to_string(),
        description: None,
        import_path: None,
        is_public: false,
        is_type_only: false,
        uri: Url::from_file_path(&frag_b_path).unwrap(),
        package_root: Some(base.clone()), // Use base as root
        used_variables: Vec::new(),
        used_fragments: Vec::new(),
        requirements: std::collections::BTreeMap::new(),
    };

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[other_frag], None, None, false, true);

    assert_eq!(diagnostics.len(), 1);
    let diag = &diagnostics[0];
    assert_eq!(
        diag.message,
        "Duplicate fragment name: 'DuplicateFrag' in the same project."
    );
    crate::support::assert_diag_range_equals(
        diag,
        &crate::support::range_for_token(&doc, text_a, "DuplicateFrag"),
    );
}

#[test]
#[ntest::timeout(100)]
fn test_private_duplicate_same_project_via_config_reports_error() {
    let dir = tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    std::fs::write(base.join("package.json"), "{}").unwrap();

    let pkg_a = base.join("pkg_a");
    let pkg_b = base.join("pkg_b");
    std::fs::create_dir_all(&pkg_a).unwrap();
    std::fs::create_dir_all(&pkg_b).unwrap();

    let frag_a_path = pkg_a.join("a.graphql");
    let frag_b_path = pkg_b.join("b.graphql");

    let text_a = "fragment DuplicateFrag on User { id }";
    std::fs::write(&frag_a_path, text_a).unwrap();
    std::fs::write(&frag_b_path, "fragment DuplicateFrag on User { name }").unwrap();

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let doc = create_doc(uri_a.as_str(), text_a);

    let schema = Schema::parse(
        "type User { id: ID! name: String } type Query { me: User }",
        "schema.graphql",
    )
    .unwrap()
    .validate()
    .unwrap();

    // Build a Config that maps both files to the same project include
    let config = graphql_rust::Config {
        output_dir: None,
        projects: vec![graphql_rust::config::ProjectConfig {
            schema: graphql_rust::config::SchemaSource::Single("schema.graphql".to_string()),
            include: graphql_rust::config::GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        base_dir: base.clone(),
        ..graphql_rust::Config::new_empty()
    };

    let other_frag = FragmentCompletionInfo {
        name: "DuplicateFrag".to_string(),
        type_condition: "User".to_string(),
        description: None,
        import_path: None,
        is_public: false,
        is_type_only: false,
        uri: Url::from_file_path(&frag_b_path).unwrap(),
        package_root: Some(base.clone()), // same package root
        used_variables: Vec::new(),
        used_fragments: Vec::new(),
        requirements: std::collections::BTreeMap::new(),
    };

    let diagnostics =
        doc.get_semantic_diagnostics(&schema, &[other_frag], None, Some(&config), false, true);

    assert_eq!(diagnostics.len(), 1);
    let diag = &diagnostics[0];
    assert_eq!(
        diag.message,
        "Duplicate fragment name: 'DuplicateFrag' in the same project."
    );
    crate::support::assert_diag_range_equals(
        diag,
        &crate::support::range_for_token(&doc, text_a, "DuplicateFrag"),
    );
}

#[test]
#[ntest::timeout(100)]
fn test_public_duplicate_across_workspace_reports_error() {
    let dir = tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let pkg_a = base.join("pkg_a");
    let pkg_b = base.join("pkg_b");
    std::fs::create_dir_all(&pkg_a).unwrap();
    std::fs::create_dir_all(&pkg_b).unwrap();
    std::fs::write(base.join("package.json"), "{}").unwrap();

    let frag_a_path = pkg_a.join("a.graphql");
    let frag_b_path = pkg_b.join("b.graphql");

    let text_a = "fragment PublicFrag on User @public { id }";
    std::fs::write(&frag_a_path, text_a).unwrap();
    std::fs::write(&frag_b_path, "fragment PublicFrag on User @public { name }").unwrap();

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let doc = create_doc(uri_a.as_str(), text_a);

    let schema = Schema::parse(
        "type User { id: ID! name: String } type Query { me: User }",
        "schema.graphql",
    )
    .unwrap()
    .validate()
    .unwrap();

    let other_frag = FragmentCompletionInfo {
        name: "PublicFrag".to_string(),
        type_condition: "User".to_string(),
        description: None,
        import_path: None,
        is_public: true,
        is_type_only: false,
        uri: Url::from_file_path(&frag_b_path).unwrap(),
        package_root: Some(base.clone()),
        used_variables: Vec::new(),
        used_fragments: Vec::new(),
        requirements: std::collections::BTreeMap::new(),
    };

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[other_frag], None, None, false, true);

    assert_eq!(diagnostics.len(), 1);
    let diag = &diagnostics[0];
    assert_eq!(
        diag.message,
        "Duplicate public fragment name: 'PublicFrag'. Public fragments must have unique names across the workspace."
    );
    crate::support::assert_diag_range_equals(
        diag,
        &crate::support::range_for_token(&doc, text_a, "PublicFrag"),
    );
}

#[test]
#[ntest::timeout(100)]
fn test_private_shadows_public_emits_hint() {
    let dir = tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let pkg_a = base.join("pkg_a");
    let pkg_b = base.join("pkg_b");
    std::fs::create_dir_all(&pkg_a).unwrap();
    std::fs::create_dir_all(&pkg_b).unwrap();
    std::fs::write(base.join("package.json"), "{}").unwrap();

    let frag_a_path = pkg_a.join("a.graphql");
    let frag_b_path = pkg_b.join("b.graphql");

    let text_a = "fragment Shadowed on User { id }";
    std::fs::write(&frag_a_path, text_a).unwrap();
    std::fs::write(&frag_b_path, "fragment Shadowed on User @public { name }").unwrap();

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let doc = create_doc(uri_a.as_str(), text_a);

    let schema = Schema::parse(
        "type User { id: ID! name: String } type Query { me: User }",
        "schema.graphql",
    )
    .unwrap()
    .validate()
    .unwrap();

    let public_frag = FragmentCompletionInfo {
        name: "Shadowed".to_string(),
        type_condition: "User".to_string(),
        description: None,
        import_path: None,
        is_public: true,
        is_type_only: false,
        uri: Url::from_file_path(&frag_b_path).unwrap(),
        package_root: Some(pkg_b.clone()),
        used_variables: Vec::new(),
        used_fragments: Vec::new(),
        requirements: std::collections::BTreeMap::new(),
    };

    let diagnostics =
        doc.get_semantic_diagnostics(&schema, &[public_frag], None, None, false, true);

    assert_eq!(diagnostics.len(), 1);
    let diag = &diagnostics[0];
    assert!(diag.message.contains("shadows a public fragment"));
    assert_eq!(diag.severity, Some(DiagnosticSeverity::HINT));
    crate::support::assert_diag_range_equals(
        diag,
        &crate::support::range_for_token(&doc, text_a, "Shadowed"),
    );
}

#[test]
#[ntest::timeout(100)]
fn test_private_duplicates_across_different_projects_do_not_error() {
    let dir = tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let pkg_a = base.join("pkg_a");
    let pkg_b = base.join("pkg_b");
    std::fs::create_dir_all(&pkg_a).unwrap();
    std::fs::create_dir_all(&pkg_b).unwrap();
    // No shared package.json here intentionally so they have different roots
    std::fs::write(pkg_a.join("package.json"), "{}").unwrap();
    std::fs::write(pkg_b.join("package.json"), "{}").unwrap();

    let frag_a_path = pkg_a.join("a.graphql");
    let frag_b_path = pkg_b.join("b.graphql");

    std::fs::write(&frag_a_path, "fragment DuplicateFrag on User { id }").unwrap();
    std::fs::write(&frag_b_path, "fragment DuplicateFrag on User { name }").unwrap();

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

    let other_frag = FragmentCompletionInfo {
        name: "DuplicateFrag".to_string(),
        type_condition: "User".to_string(),
        description: None,
        import_path: None,
        is_public: false,
        is_type_only: false,
        uri: Url::from_file_path(&frag_b_path).unwrap(),
        package_root: Some(pkg_b.clone()),
        used_variables: Vec::new(),
        used_fragments: Vec::new(),
        requirements: std::collections::BTreeMap::new(),
    };

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[other_frag], None, None, false, true);

    assert!(
        diagnostics.is_empty(),
        "Did not expect duplicate fragment error across different projects, got: {:?}",
        diagnostics
    );
}
