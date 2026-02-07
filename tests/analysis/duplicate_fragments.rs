use crate::support::create_doc;
use apollo_compiler::Schema;
use graphql_rust::features::completion::FragmentCompletionInfo;
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

    std::fs::write(&frag_a_path, "fragment DuplicateFrag on User { id }").unwrap();
    std::fs::write(&frag_b_path, "fragment DuplicateFrag on User { name }").unwrap();

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let doc = create_doc(uri_a.as_str(), &std::fs::read_to_string(&frag_a_path).unwrap());

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

    let expected_message =
        "Duplicate fragment name: 'DuplicateFrag' in the same project.".to_string();
    let diag = diagnostics
        .iter()
        .find(|d| d.message == expected_message)
        .unwrap_or_else(|| panic!("Expected duplicate fragment error, got: {:?}", diagnostics));

    // Range should point at fragment name in this document
    let last_name = "DuplicateFrag";
    let text = std::fs::read_to_string(&frag_a_path).unwrap();
    let expected = crate::support::range_for_token(&doc, &text, last_name);
    assert_eq!(diag.range.start, expected.start);
    assert_eq!(diag.range.end, expected.end);
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

    std::fs::write(&frag_a_path, "fragment DuplicateFrag on User { id }").unwrap();
    std::fs::write(&frag_b_path, "fragment DuplicateFrag on User { name }").unwrap();

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let doc = create_doc(uri_a.as_str(), &std::fs::read_to_string(&frag_a_path).unwrap());

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

    let expected_message =
        "Duplicate fragment name: 'DuplicateFrag' in the same project.".to_string();
    let diag = diagnostics
        .iter()
        .find(|d| d.message == expected_message)
        .unwrap_or_else(|| {
            panic!(
                "Expected duplicate fragment error with project config, got: {:?}",
                diagnostics
            )
        });

    assert_eq!(diag.range.start.line, 0);
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

    std::fs::write(&frag_a_path, "fragment PublicFrag on User @public { id }").unwrap();
    std::fs::write(&frag_b_path, "fragment PublicFrag on User @public { name }").unwrap();

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let doc = create_doc(uri_a.as_str(), &std::fs::read_to_string(&frag_a_path).unwrap());

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

    let expected_message =
        "Duplicate public fragment name: 'PublicFrag'. Public fragments must have unique names across the workspace."
            .to_string();
    let diag = diagnostics
        .iter()
        .find(|d| d.message == expected_message)
        .unwrap_or_else(|| {
            panic!(
                "Expected public duplicate error across workspace, got: {:?}",
                diagnostics
            )
        });

    let expected_range = crate::support::range_for_token(&doc, &std::fs::read_to_string(&frag_a_path).unwrap(), "PublicFrag");
    crate::support::assert_diag_range_equals(diag, &expected_range);
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

    std::fs::write(&frag_a_path, "fragment Shadowed on User { id }").unwrap();
    std::fs::write(&frag_b_path, "fragment Shadowed on User @public { name }").unwrap();

    let uri_a = Url::from_file_path(&frag_a_path).unwrap();
    let doc = create_doc(uri_a.as_str(), &std::fs::read_to_string(&frag_a_path).unwrap());

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

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[public_frag], None, None, false, true);

    let found = diagnostics.iter().any(|d| {
        d.message.contains("shadows a public fragment")
            && d.severity == Some(DiagnosticSeverity::HINT)
    });
    assert!(found, "Expected shadowing hint, got: {:?}", diagnostics);
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
    let doc = create_doc(uri_a.as_str(), &std::fs::read_to_string(&frag_a_path).unwrap());

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