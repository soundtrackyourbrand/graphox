use apollo_compiler::Schema;
use graphql_rust::features::completion::FragmentCompletionInfo;
use graphql_rust::{Backend, Config, DocumentState};
use std::fs;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

fn create_parser() -> tree_sitter::Parser {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();
    parser
}

#[test]
#[ntest::timeout(1000)]
fn test_diagnostics_update_on_schema_change() {
    let schema_v1_content = "type User { id: ID! name: String } type Query { me: User }";
    let schema_v1 = Schema::parse(schema_v1_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let query_text = "query { me { id name } }";
    let uri = Url::parse("file:///query.graphql").unwrap();
    let doc = DocumentState::new(uri.clone(), query_text, create_parser());

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
    assert!(diagnostics[0].message.contains("name"));

    // Query fixed: use 'fullName'
    let query_text_v2 = "query { me { id fullName } }";
    let doc_v2 = DocumentState::new(uri, query_text_v2, create_parser());
    let diagnostics = doc_v2.get_semantic_diagnostics(&schema_v2, &[], None, None, false, true);
    assert!(diagnostics.is_empty(), "Should be valid after fixing query");
}

#[test]
#[ntest::timeout(1000)]
fn test_diagnostics_update_on_fragment_change() {
    let schema_content = "type User { id: ID! name: String } type Query { me: User }";
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let query_text = "query { me { ...UserFrag } }";
    let query_uri = Url::parse("file:///query.graphql").unwrap();
    let query_doc = DocumentState::new(query_uri, query_text, create_parser());

    // 1. Missing fragment
    let diagnostics = query_doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    assert!(!diagnostics.is_empty());
    assert!(
        diagnostics[0]
            .message
            .contains("Unknown fragment: UserFrag")
    );

    // 2. Fragment provided (simulating it being found in another file)
    let fragments = vec![FragmentCompletionInfo {
        name: "UserFrag".to_string(),
        type_condition: "User".to_string(),
        description: None,
        import_path: None,
        is_public: false,
        is_type_only: false,
        uri: Url::parse("file:///frag.graphql").unwrap(),
        package_root: None,
        used_variables: Vec::new(),
        used_fragments: Vec::new(),
        requirements: std::collections::BTreeMap::new(),
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
        name: "UserFragRenamed".to_string(),
        type_condition: "User".to_string(),
        description: None,
        import_path: None,
        is_public: false,
        is_type_only: false,
        uri: Url::parse("file:///frag.graphql").unwrap(),
        package_root: None,
        used_variables: Vec::new(),
        used_fragments: Vec::new(),
        requirements: std::collections::BTreeMap::new(),
    }];
    let diagnostics =
        query_doc.get_semantic_diagnostics(&schema, &fragments, None, None, false, true);
    assert!(!diagnostics.is_empty());
    assert!(
        diagnostics[0]
            .message
            .contains("Unknown fragment: UserFrag")
    );
}

#[tokio::test]
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
