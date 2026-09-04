use apollo_compiler::Schema;
use graphox::features::completion::FragmentCompletionInfo;
use graphox::features::diagnostics::DocumentDiagnostics;
use graphox::{Backend, Config};
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;
use tower_lsp_server::LspService;
use tower_lsp_server::jsonrpc::Request;
use tower_lsp_server::ls_types::*;
use tower_service::Service;

use crate::support::create_doc;

#[test]
#[ntest::timeout(3000)]
fn test_diagnostics_update_on_schema_change() {
    let schema_v1_content = "type User { id: ID! name: String } type Query { me: User }";
    let schema_v1 = Schema::parse(schema_v1_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let query_text = "query { me { id name } }";
    let doc = create_doc("file:///query.graphql", query_text);

    // Initially valid
    let diagnostics = doc.get_semantic_diagnostics(&schema_v1, &[], None, None, false, true);
    assert!(
        diagnostics.is_empty(),
        "Initially should be valid, got {:?}",
        diagnostics
    );

    // Schema change: rename 'name' to 'fullName'
    let schema_v2_content = "type User { id: ID! fullName: String } type Query { me: User }";
    let schema_v2 = Schema::parse(schema_v2_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    // Now should have diagnostics
    let diagnostics = doc.get_semantic_diagnostics(&schema_v2, &[], None, None, false, true);
    assert!(
        !diagnostics.is_empty(),
        "Should have diagnostics after schema change"
    );
    // Expect deterministic internal message
    let msg = diagnostics[0].message.clone();
    assert_eq!(
        msg, "Field 'name' not found on type 'User'",
        "Unexpected diagnostic message: {}",
        msg
    );

    // Query fixed: use 'fullName'
    let query_text_v2 = "query { me { id fullName } }";
    let doc_v2 = create_doc("file:///query.graphql", query_text_v2);
    let diagnostics = doc_v2.get_semantic_diagnostics(&schema_v2, &[], None, None, false, true);
    assert!(diagnostics.is_empty(), "Should be valid after fixing query");
}

#[test]
#[ntest::timeout(3000)]
fn test_diagnostics_update_on_fragment_change() {
    let schema_content = "type User { id: ID! name: String } type Query { me: User }";
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let query_text = "query { me { ...UserFrag } }";
    let query_uri = "file:///query.graphql".parse::<Uri>().unwrap();
    let query_doc = create_doc(query_uri.as_str(), query_text);

    // 1. Missing fragment
    let diagnostics = query_doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    assert!(!diagnostics.is_empty());
    assert_eq!(diagnostics[0].message, "Unknown fragment: UserFrag");

    // 2. Fragment provided (simulating it being found in another file)
    let fragments = vec![FragmentCompletionInfo {
        name: "UserFrag".into(),
        type_condition: "User".into(),
        description: None,
        import_path: None,
        is_public: false,
        is_type_only: false,
        uri: "file:///frag.graphql".parse::<Uri>().unwrap(),
        package_root: None,
        used_variables: Arc::from([]),
        used_fragments: Arc::from([]),
        transitive_deps: Arc::from([]),
        selected_fields: Arc::from([]),
        top_level_spreads: Arc::from([]),
        nested_selections: Arc::from([]),
        selection_ignores: Arc::from([]),
        spread_ignores: Arc::from([]),
        type_fields: Arc::from([]),
        requirements: std::collections::BTreeMap::new(),
        worst_slo: None,
    }];
    let diagnostics =
        query_doc.get_semantic_diagnostics(&schema, &fragments, None, None, false, true);
    assert!(
        diagnostics.is_empty(),
        "Should be valid when fragment is provided, got {:?}",
        diagnostics
    );

    // 3. Fragment renamed (simulating rename in another file)
    let fragments = vec![FragmentCompletionInfo {
        name: "UserFragRenamed".into(),
        type_condition: "User".into(),
        description: None,
        import_path: None,
        is_public: false,
        is_type_only: false,
        uri: "file:///frag.graphql".parse::<Uri>().unwrap(),
        package_root: None,
        used_variables: Arc::from([]),
        used_fragments: Arc::from([]),
        transitive_deps: Arc::from([]),
        selected_fields: Arc::from([]),
        top_level_spreads: Arc::from([]),
        nested_selections: Arc::from([]),
        selection_ignores: Arc::from([]),
        spread_ignores: Arc::from([]),
        type_fields: Arc::from([]),
        requirements: std::collections::BTreeMap::new(),
        worst_slo: None,
    }];
    let diagnostics =
        query_doc.get_semantic_diagnostics(&schema, &fragments, None, None, false, true);
    assert!(!diagnostics.is_empty());
    assert_eq!(diagnostics[0].message, "Unknown fragment: UserFrag");
}

#[test]
#[ntest::timeout(3000)]
fn test_incremental_fragment_removal_falls_back_to_public_fragment() {
    let schema_content = "type User { id: ID! name: String } type Query { me: User }";
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let local_uri = "file:///pkg_b/local.graphql".parse::<Uri>().unwrap();
    let query_uri = "file:///pkg_b/query.graphql".parse::<Uri>().unwrap();
    let local_text = "fragment UserFields on User { name }";
    let query_text = "query { me { ...UserFields } }";

    let mut local_doc = create_doc(local_uri.as_str(), local_text);
    let query_doc = create_doc(query_uri.as_str(), query_text);

    let public_fragment = FragmentCompletionInfo {
        name: "UserFields".into(),
        type_condition: "User".into(),
        description: None,
        import_path: None,
        is_public: true,
        is_type_only: false,
        uri: "file:///pkg_a/public.graphql".parse::<Uri>().unwrap(),
        package_root: Some(std::path::PathBuf::from("/pkg_a")),
        used_variables: Arc::from([]),
        used_fragments: Arc::from([]),
        transitive_deps: Arc::from([]),
        selected_fields: Arc::from([Arc::<str>::from("id")]),
        top_level_spreads: Arc::from([]),
        nested_selections: Arc::from([]),
        selection_ignores: Arc::from([]),
        spread_ignores: Arc::from([]),
        type_fields: Arc::from([]),
        requirements: std::collections::BTreeMap::new(),
        worst_slo: None,
    };

    let local_fragment = FragmentCompletionInfo {
        name: "UserFields".into(),
        type_condition: "User".into(),
        description: None,
        import_path: None,
        is_public: false,
        is_type_only: false,
        uri: local_uri.clone(),
        package_root: Some(std::path::PathBuf::from("/pkg_b")),
        used_variables: Arc::from([]),
        used_fragments: Arc::from([]),
        transitive_deps: Arc::from([]),
        selected_fields: Arc::from([Arc::<str>::from("name")]),
        top_level_spreads: Arc::from([]),
        nested_selections: Arc::from([]),
        selection_ignores: Arc::from([]),
        spread_ignores: Arc::from([]),
        type_fields: Arc::from([]),
        requirements: std::collections::BTreeMap::new(),
        worst_slo: None,
    };

    let diagnostics = query_doc.get_semantic_diagnostics(
        &schema,
        &[local_fragment.clone(), public_fragment.clone()],
        None,
        None,
        false,
        true,
    );
    assert!(
        diagnostics.is_empty(),
        "Query should initially resolve the local fragment"
    );

    let change = TextDocumentContentChangeEvent {
        range: Some(Range {
            start: Position::new(0, 0),
            end: Position::new(0, local_text.len() as u32),
        }),
        range_length: None,
        text: "# local fragment deleted".to_string(),
    };
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&graphox::DocumentLanguage::GraphQL.get_parser_language())
        .unwrap();
    local_doc.apply_change(&change, &mut parser, 2);

    assert!(
        local_doc.fragments().is_empty(),
        "Incremental edit should remove the local fragment definition"
    );

    let diagnostics =
        query_doc.get_semantic_diagnostics(&schema, &[public_fragment], None, None, false, true);
    assert!(
        diagnostics.is_empty(),
        "Query should fall back to the public fragment after incremental removal, got {diagnostics:?}"
    );
}

#[tokio::test]
#[ntest::timeout(5000)]
async fn test_backend_schema_reload() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(&schema_path, "type Query { me: String }").unwrap();

    let (mut service, _) = LspService::new(|client| {
        graphox::GraphoxLanguageServer::new(Backend::new(client, Config::new_empty()))
    });

    let params = DidChangeWatchedFilesParams {
        changes: vec![FileEvent {
            uri: graphox::utils::path_to_uri(&schema_path).unwrap(),
            typ: FileChangeType::CHANGED,
        }],
    };

    let request = Request::build("workspace/didChangeWatchedFiles")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let _ = service.call(request).await.unwrap();
}
