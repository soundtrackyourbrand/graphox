use apollo_compiler::Schema;
use graphql_rust::features::completion::FragmentCompletionInfo;
use graphql_rust::DocumentState;
use std::path::PathBuf;
use tempfile::tempdir;
use tower_lsp::lsp_types::*;

fn create_doc(uri_str: &str, text: &str) -> DocumentState {
    let uri = Url::parse(uri_str).unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();
    DocumentState::new(uri, text, parser)
}

#[test]
#[ntest::timeout(100)]
fn test_private_duplicate_same_package_root_reports_error() {
    let dir = tempdir().unwrap();
    let pkg = dir.path().join("pkg");
    std::fs::create_dir_all(&pkg).unwrap();

    let frag_a_path = pkg.join("a.graphql");
    let frag_b_path = pkg.join("b.graphql");

    std::fs::write(&frag_a_path, "fragment DuplicateFrag on User { id }").unwrap();
    std::fs::write(&frag_b_path, "fragment DuplicateFrag on User { name }").unwrap();

    let doc = create_doc(
        &format!("file://{}", frag_a_path.to_string_lossy()),
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
        package_root: Some(pkg.clone()),
        used_variables: Vec::new(),
        used_fragments: Vec::new(),
        requirements: std::collections::BTreeMap::new(),
    };

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[other_frag], None, None, false, true);

    assert!(
        diagnostics.iter().any(|d| d
            .message
            .contains("Duplicate fragment name: 'DuplicateFrag'")),
        "Expected duplicate fragment error, got: {:?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(100)]
fn test_private_duplicate_same_project_via_config_reports_error() {
    let dir = tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();

    let pkg_a = base.join("pkg_a");
    let pkg_b = base.join("pkg_b");
    std::fs::create_dir_all(&pkg_a).unwrap();
    std::fs::create_dir_all(&pkg_b).unwrap();

    let frag_a_path = pkg_a.join("a.graphql");
    let frag_b_path = pkg_b.join("b.graphql");

    std::fs::write(&frag_a_path, "fragment DuplicateFrag on User { id }").unwrap();
    std::fs::write(&frag_b_path, "fragment DuplicateFrag on User { name }").unwrap();

    let doc = create_doc(
        &format!("file://{}", frag_a_path.to_string_lossy()),
        &std::fs::read_to_string(&frag_a_path).unwrap(),
    );

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
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        watch_all_files: None,
        enable_schema_cache: Some(true),
        base_dir: base.clone(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        timeouts: None,
        rules: None,
    };

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

    let diagnostics =
        doc.get_semantic_diagnostics(&schema, &[other_frag], None, Some(&config), false, true);

    assert!(
        diagnostics.iter().any(|d| d
            .message
            .contains("Duplicate fragment name: 'DuplicateFrag'")),
        "Expected duplicate fragment error with project config, got: {:?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(100)]
fn test_public_duplicate_across_workspace_reports_error() {
    let dir = tempdir().unwrap();
    let pkg_a = dir.path().join("pkg_a");
    let pkg_b = dir.path().join("pkg_b");
    std::fs::create_dir_all(&pkg_a).unwrap();
    std::fs::create_dir_all(&pkg_b).unwrap();

    let frag_a_path = pkg_a.join("a.graphql");
    let frag_b_path = pkg_b.join("b.graphql");

    std::fs::write(&frag_a_path, "fragment PublicFrag on User @public { id }").unwrap();
    std::fs::write(&frag_b_path, "fragment PublicFrag on User @public { name }").unwrap();

    let doc = create_doc(
        &format!("file://{}", frag_a_path.to_string_lossy()),
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
        name: "PublicFrag".to_string(),
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

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[other_frag], None, None, false, true);

    assert!(
        diagnostics.iter().any(|d| d
            .message
            .contains("Duplicate public fragment name: 'PublicFrag'")),
        "Expected duplicate public fragment error, got: {:?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(100)]
fn test_private_shadows_public_emits_hint() {
    let dir = tempdir().unwrap();
    let pkg_a = dir.path().join("pkg_a");
    let pkg_b = dir.path().join("pkg_b");
    std::fs::create_dir_all(&pkg_a).unwrap();
    std::fs::create_dir_all(&pkg_b).unwrap();

    let frag_private_path = pkg_a.join("private.graphql");
    let frag_public_path = pkg_b.join("public.graphql");

    std::fs::write(&frag_private_path, "fragment PublicFrag on User { id }").unwrap();
    std::fs::write(
        &frag_public_path,
        "fragment PublicFrag on User @public { id }",
    )
    .unwrap();

    let doc = create_doc(
        &format!("file://{}", frag_private_path.to_string_lossy()),
        &std::fs::read_to_string(&frag_private_path).unwrap(),
    );

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
        uri: Url::from_file_path(&frag_public_path).unwrap(),
        package_root: Some(pkg_b.clone()),
        used_variables: Vec::new(),
        used_fragments: Vec::new(),
        requirements: std::collections::BTreeMap::new(),
    };

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[other_frag], None, None, false, true);

    assert!(
        diagnostics.iter().any(|d| d.severity
            == Some(tower_lsp::lsp_types::DiagnosticSeverity::HINT)
            && d.message.contains("shadows a public fragment")),
        "Expected hint about shadowing public fragment, got: {:?}",
        diagnostics
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

    let frag_a_path = pkg_a.join("a.graphql");
    let frag_b_path = pkg_b.join("b.graphql");

    std::fs::write(&frag_a_path, "fragment DuplicateFrag on User { id }").unwrap();
    std::fs::write(&frag_b_path, "fragment DuplicateFrag on User { name }").unwrap();

    let doc = create_doc(
        &format!("file://{}", frag_a_path.to_string_lossy()),
        &std::fs::read_to_string(&frag_a_path).unwrap(),
    );

    let schema = Schema::parse(
        "type User { id: ID! name: String } type Query { me: User }",
        "schema.graphql",
    )
    .unwrap()
    .validate()
    .unwrap();

    // Two different projects
    let config = graphql_rust::Config {
        output_dir: None,
        projects: vec![
            graphql_rust::config::ProjectConfig {
                schema: graphql_rust::config::SchemaSource::Single("schema.graphql".to_string()),
                include: graphql_rust::config::GlobPattern::Single(
                    "pkg_a/**/*.graphql".to_string(),
                ),
                exclude: None,
                output_dir: None,
                import: None,
                generate_permissions: None,
                codegen: Some(false),
            },
            graphql_rust::config::ProjectConfig {
                schema: graphql_rust::config::SchemaSource::Single("schema.graphql".to_string()),
                include: graphql_rust::config::GlobPattern::Single(
                    "pkg_b/**/*.graphql".to_string(),
                ),
                exclude: None,
                output_dir: None,
                import: None,
                generate_permissions: None,
                codegen: Some(false),
            },
        ],
        schema_types: None,
        scalars: None,
        ignore_deprecations: None,
        generate_ast_for_fragments: None,
        tracing: None,
        watch_all_files: None,
        enable_schema_cache: Some(true),
        base_dir: base.clone(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        timeouts: None,
        rules: None,
    };

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

    let diagnostics =
        doc.get_semantic_diagnostics(&schema, &[other_frag], None, Some(&config), false, true);

    assert!(
        !diagnostics.iter().any(|d| d
            .message
            .contains("Duplicate fragment name: 'DuplicateFrag'")),
        "Did not expect duplicate fragment error across different projects, got: {:?}",
        diagnostics
    );
}
