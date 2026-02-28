use apollo_compiler::Schema;
use graphox::features::completion::FragmentCompletionInfo;
use graphox::features::diagnostics::DocumentDiagnostics;
use graphox::{Backend, Config};
use std::fs;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

use crate::support::create_doc;

#[test]
#[ntest::timeout(100)]
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
#[ntest::timeout(100)]
fn test_diagnostics_update_on_fragment_change() {
    let schema_content = "type User { id: ID! name: String } type Query { me: User }";
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let query_text = "query { me { ...UserFrag } }";
    let query_uri = Url::parse("file:///query.graphql").unwrap();
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
        uri: Url::parse("file:///frag.graphql").unwrap(),
        package_root: None,
        used_variables: Vec::new(),
        used_fragments: Vec::new(),
        transitive_deps: Vec::new(),
        selected_fields: Vec::new(),
        type_fields: Vec::new(),
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
        uri: Url::parse("file:///frag.graphql").unwrap(),
        package_root: None,
        used_variables: Vec::new(),
        used_fragments: Vec::new(),
        transitive_deps: Vec::new(),
        selected_fields: Vec::new(),
        type_fields: Vec::new(),
        requirements: std::collections::BTreeMap::new(),
        worst_slo: None,
    }];
    let diagnostics =
        query_doc.get_semantic_diagnostics(&schema, &fragments, None, None, false, true);
    assert!(!diagnostics.is_empty());
    assert_eq!(diagnostics[0].message, "Unknown fragment: UserFrag");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_backend_schema_reload() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(&schema_path, "type Query { me: String }").unwrap();

    let (mut service, _) = LspService::new(|client| Backend::new(client, Config::new_empty()));

    let params = DidChangeWatchedFilesParams {
        changes: vec![FileEvent {
            uri: Url::from_file_path(&schema_path).unwrap(),
            typ: FileChangeType::CHANGED,
        }],
    };

    let request = Request::build("workspace/didChangeWatchedFiles")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let _ = service.call(request).await.unwrap();
}
