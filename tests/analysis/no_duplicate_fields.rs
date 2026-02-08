use crate::support::{
    create_doc, create_initialized_lsp_service, find_code_action_by_title, lsp_did_open,
    lsp_request_code_actions, make_temp_project_with_schema, range_for_token_at_index,
    write_project_file,
};
use graphql_rust::config::{GlobPattern, ProjectConfig, SchemaSource};
use graphql_rust::Config;
use std::fs;
use tempfile::tempdir;
use tower_lsp::lsp_types::*;

// Basic analysis test: shallow duplicate fields
#[test]
#[ntest::timeout(5000)]
fn test_shallow_duplicate_fields_check() {
    let dir = tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();

    fs::write(base.join("package.json"), "{}").unwrap();
    fs::write(
        base.join("schema.graphql"),
        "type Query { me: User } type User { id: ID name: String }",
    )
    .unwrap();

    let query_text = "query { me { id id name } }";
    let q_path = base.join("q.graphql");
    fs::write(&q_path, query_text).unwrap();

    let config = Config {
        base_dir: base.clone(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None,
        }],
        rules: Some(Default::default()),
        ..Default::default()
    };

    // Enable the rule
    let mut cfg = config;
    if let Some(rules) = &mut cfg.rules {
        rules.no_duplicate_fields = Some(true);
    }

    let schema_text = fs::read_to_string(base.join("schema.graphql")).unwrap();
    let schema = apollo_compiler::Schema::parse(&schema_text, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let uri = Url::from_file_path(&q_path).unwrap();
    let doc = create_doc(uri.as_str(), query_text);
    let diags = doc.get_semantic_diagnostics(&schema, &[], None, Some(&cfg), false, true);

    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.message, "Duplicate field 'id' in selection set");
    // range points to the second 'id'
    crate::support::assert_diag_range_equals(d, &range_for_token_at_index(&doc, query_text, "id", 1));
}

// Canonicalization test: arg order variations should be reported
#[test]
#[ntest::timeout(5000)]
fn test_duplicate_fields_with_different_arg_order_are_reported() {
    let dir = tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();

    fs::write(base.join("package.json"), "{}").unwrap();
    fs::write(
        base.join("schema.graphql"),
        "type Query { me(id: ID, other: Int): User } type User { id: ID name: String }",
    )
    .unwrap();

    let query_text = "query GetMe($id: ID) { me(id: $id, other: 2) { id } me(other: 2, id: $id) { id } }";
    let q_path = base.join("q.graphql");
    fs::write(&q_path, query_text).unwrap();

    let config = Config {
        base_dir: base.clone(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None,
        }],
        rules: Some(Default::default()),
        ..Default::default()
    };

    // Enable the rule
    let mut cfg = config;
    if let Some(rules) = &mut cfg.rules {
        rules.no_duplicate_fields = Some(true);
    }

    let schema_text = fs::read_to_string(base.join("schema.graphql")).unwrap();
    let schema = apollo_compiler::Schema::parse(&schema_text, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let uri = Url::from_file_path(&q_path).unwrap();
    let doc = create_doc(uri.as_str(), query_text);
    let diags = doc.get_semantic_diagnostics(&schema, &[], None, Some(&cfg), false, true);

    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.message, "Duplicate field 'me' in selection set");
    // range points to the second 'me'
    crate::support::assert_diag_range_equals(d, &range_for_token_at_index(&doc, query_text, "me", 1));
}

// Alias handling tests
#[test]
#[ntest::timeout(5000)]
fn test_duplicate_fields_with_alias_handling() {
    let (dir, mut cfg) = make_temp_project_with_schema(
        "type Query { me: User } type User { id: ID name: String }",
        "**/*.graphql",
    );

    cfg.rules = Some(Default::default());
    if let Some(rules) = &mut cfg.rules {
        rules.no_duplicate_fields = Some(true);
    }

    let schema_text = fs::read_to_string(dir.path().join("schema.graphql")).unwrap();
    let schema = apollo_compiler::Schema::parse(&schema_text, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    // 1. OK case
    let q1_text = "query { me { a: id id name } }";
    let uri1 = write_project_file(&dir, "q1.graphql", q1_text);
    let doc1 = create_doc(uri1.as_str(), q1_text);
    let diags1 = doc1.get_semantic_diagnostics(&schema, &[], None, Some(&cfg), false, true);
    assert_eq!(diags1.len(), 0);

    // 2. Duplicate alias
    let q2_text = "query { me { a: id a: id } }";
    let uri2 = write_project_file(&dir, "q2.graphql", q2_text);
    let doc2 = create_doc(uri2.as_str(), q2_text);
    let diags2 = doc2.get_semantic_diagnostics(&schema, &[], None, Some(&cfg), false, true);
    
    assert_eq!(diags2.len(), 1);
    let d = &diags2[0];
    assert_eq!(d.message, "Duplicate field 'a' in selection set");
    // points to second 'id' (the field for the duplicate alias 'a')
    crate::support::assert_diag_range_equals(d, &range_for_token_at_index(&doc2, q2_text, "id", 1));
}

// Alias collisions: alias name equals an unaliased field -> should trigger
#[test]
#[ntest::timeout(5000)]
fn test_alias_collision_triggers_duplicate() {
    let (dir, mut cfg) = make_temp_project_with_schema(
        "type Query { me: User } type User { id: ID name: String }",
        "**/*.graphql",
    );

    cfg.rules = Some(Default::default());
    if let Some(rules) = &mut cfg.rules {
        rules.no_duplicate_fields = Some(true);
    }

    let schema_text = fs::read_to_string(dir.path().join("schema.graphql")).unwrap();
    let schema = apollo_compiler::Schema::parse(&schema_text, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let query_text = "query { me { id name id: name } }";
    let uri = write_project_file(&dir, "q.graphql", query_text);
    let doc = create_doc(uri.as_str(), query_text);
    let diags = doc.get_semantic_diagnostics(&schema, &[], None, Some(&cfg), false, true);

    assert_eq!(diags.len(), 1);
    let d = &diags[0];
    assert_eq!(d.message, "Duplicate field 'id' in selection set");
    // points to 'name' (the field for the alias 'id' that collisions with first 'id')
    crate::support::assert_diag_range_equals(d, &range_for_token_at_index(&doc, query_text, "name", 1));
}

// Tests involving fragments and inline fragments
#[test]
#[ntest::timeout(5000)]
fn test_duplicate_fields_with_fragments_and_inline_fragments() {
    let (dir, mut cfg) = make_temp_project_with_schema(
        "type Query { me: User } type User { id: ID name: String friends(limit: Int): [User] }",
        "**/*.graphql",
    );

    cfg.rules = Some(Default::default());
    if let Some(rules) = &mut cfg.rules {
        rules.no_duplicate_fields = Some(true);
    }

    let schema_text = fs::read_to_string(dir.path().join("schema.graphql")).unwrap();
    let schema = apollo_compiler::Schema::parse(&schema_text, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    // A: duplicate inside inline fragment
    let q_a = "query { me { ... on User { id id } } }";
    let uri_a = write_project_file(&dir, "a.graphql", q_a);
    let doc_a = create_doc(uri_a.as_str(), q_a);
    let diags_a = doc_a.get_semantic_diagnostics(&schema, &[], None, Some(&cfg), false, true);
    assert_eq!(diags_a.len(), 1);
    let d_a = &diags_a[0];
    assert_eq!(d_a.message, "Duplicate field 'id' in selection set");
    crate::support::assert_diag_range_equals(d_a, &range_for_token_at_index(&doc_a, q_a, "id", 1));

    // B: duplicate across inline fragment and sibling -> should NOT trigger (shallow-only)
    let q_b = "query { me { ... on User { id } id } }";
    let uri_b = write_project_file(&dir, "b.graphql", q_b);
    let doc_b = create_doc(uri_b.as_str(), q_b);
    let diags_b = doc_b.get_semantic_diagnostics(&schema, &[], None, Some(&cfg), false, true);
    assert_eq!(diags_b.len(), 0);

    // D: same response key with different args -> should trigger
    let q_d = "query GetMe($limit1: Int, $limit2: Int) { me { friends(limit: $limit1) { id } friends(limit: $limit2) { id } } }";
    let uri_d = write_project_file(&dir, "d.graphql", q_d);
    let doc_d = create_doc(uri_d.as_str(), q_d);
    let diags_d = doc_d.get_semantic_diagnostics(&schema, &[], None, Some(&cfg), false, true);
    
    // Only expect our internal diagnostic now
    assert_eq!(diags_d.len(), 1);
    let d_d = &diags_d[0];
    assert_eq!(d_d.message, "Duplicate field 'friends' in selection set");
    // range for second friends
    crate::support::assert_diag_range_equals(d_d, &range_for_token_at_index(&doc_d, q_d, "friends", 1));
}

// LSP integration test
#[tokio::test]
async fn test_alias_allowed_and_duplicate_code_action_removes_later() {
    let schema_text = "type Query { me: User } type User { id: ID name: String }";
    let (dir, config) = make_temp_project_with_schema(schema_text, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Create a document where aliases are used uniquely
    let q_alias_text = "query { me { a: id b: id name } }";
    let q_alias_uri = write_project_file(&dir, "aliases_ok.graphql", q_alias_text);
    lsp_did_open(&mut service, q_alias_uri.clone(), "graphql", 1, q_alias_text).await;

    // Create a document with a true duplicate
    let dup_text = "query { me { id id name } }";
    let dup_uri = write_project_file(&dir, "dup_action.graphql", dup_text);
    lsp_did_open(&mut service, dup_uri.clone(), "graphql", 1, dup_text).await;

    let doc_dup = create_doc(dup_uri.as_str(), dup_text);

    // Construct diagnostic pointing at duplicated `id`
    let dup_diag = Diagnostic {
        range: range_for_token_at_index(&doc_dup, dup_text, "id", 1),
        message: "Duplicate field 'id' in selection set".to_string(),
        code: Some(NumberOrString::String("no_duplicate_fields".to_string())),
        data: Some(serde_json::json!({"response_key":"id","args":"","selection":""})),
        ..Default::default()
    };

    let params_dup = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: dup_uri.clone(),
        },
        range: dup_diag.range,
        context: CodeActionContext {
            diagnostics: vec![dup_diag.clone()],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions_dup = lsp_request_code_actions(&mut service, params_dup, 2)
        .await
        .expect("Expected actions for dup file");

    let ca = find_code_action_by_title(&actions_dup, "Remove duplicate field")
        .expect("Expected 'Remove duplicate field' action");

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    let edits = &changes[&dup_uri];

    // Apply the edits using DocumentState
    let mut doc = create_doc(dup_uri.as_str(), dup_text);
    if let Some(text_edit) = edits.first() {
        let t = TextDocumentContentChangeEvent {
            range: Some(text_edit.range),
            range_length: None,
            text: text_edit.new_text.clone(),
        };
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&graphql_rust::DocumentLanguage::GraphQL.get_parser_language()).unwrap();
        doc.apply_change(&t, &mut parser, 2);

        let expected = "query { me { id name } }";
        assert_eq!(doc.rope.to_string().replace("  ", " "), expected);
    }
}