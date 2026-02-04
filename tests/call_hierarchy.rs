use graphql_rust::{
    Backend, Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
};
use std::fs;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test]
async fn test_fragment_call_hierarchy() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
        }],
        base_dir: base_dir.to_path_buf(),
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();
    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    let frag_path = base_dir.join("fragments.graphql");
    let frag_text = r#"
        fragment UserFields on User { id name }
        fragment UserWithHome on User { ...UserFields }
    "#;
    fs::write(&frag_path, frag_text).unwrap();
    let frag_path = std::fs::canonicalize(frag_path).unwrap();
    let frag_uri = Url::from_file_path(&frag_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: frag_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: frag_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // 1. Prepare Call Hierarchy on "UserFields"
    let position = Position::new(1, 17); // UserFields definition
    let params = CallHierarchyPrepareParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: frag_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
    };

    let request = Request::build("textDocument/prepareCallHierarchy")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let items: Option<Vec<CallHierarchyItem>> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let items = items.expect("Expected CallHierarchyItems");
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item.name, "UserFields");

    // 2. Test Incoming Calls (who calls UserFields?)
    let params = CallHierarchyIncomingCallsParams {
        item: item.clone(),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let request = Request::build("callHierarchy/incomingCalls")
        .id(2)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let incoming: Option<Vec<CallHierarchyIncomingCall>> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let calls = incoming.expect("Expected incoming calls");
    assert!(!calls.is_empty());
    assert!(calls.iter().any(|c| c.from.name == "UserWithHome"));

    // 3. Test Outgoing Calls for "UserWithHome"
    let position = Position::new(2, 17); // UserWithHome definition
    let params = CallHierarchyPrepareParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: frag_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
    };
    let request = Request::build("textDocument/prepareCallHierarchy")
        .id(3)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let items: Vec<CallHierarchyItem> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();
    let item_with_home = &items[0];

    let params = CallHierarchyOutgoingCallsParams {
        item: item_with_home.clone(),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let request = Request::build("callHierarchy/outgoingCalls")
        .id(4)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let outgoing: Option<Vec<CallHierarchyOutgoingCall>> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let calls = outgoing.expect("Expected outgoing calls");
    assert!(!calls.is_empty());
    assert!(calls.iter().any(|c| c.to.name == "UserFields"));
}

#[tokio::test]
async fn test_call_hierarchy_tsx() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.{graphql,tsx}".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
        }],
        base_dir: base_dir.to_path_buf(),
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();
    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    let frag_path = base_dir.join("fragments.graphql");
    let frag_text = "fragment UserFields on User { id }";
    fs::write(&frag_path, frag_text).unwrap();
    let frag_path = std::fs::canonicalize(frag_path).unwrap();
    let frag_uri = Url::from_file_path(&frag_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: frag_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: frag_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let tsx_path = base_dir.join("Component.tsx");
    let tsx_text = "const q = gql`query { me { ...UserFields } }`;";
    fs::write(&tsx_path, tsx_text).unwrap();
    let tsx_path = std::fs::canonicalize(tsx_path).unwrap();
    let tsx_uri = Url::from_file_path(&tsx_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: tsx_uri.clone(),
                            language_id: "typescriptreact".to_string(),
                            version: 1,
                            text: tsx_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // 1. Prepare Call Hierarchy on "UserFields"
    let params = CallHierarchyPrepareParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: frag_uri.clone(),
            },
            position: Position::new(0, 9),
        },
        work_done_progress_params: Default::default(),
    };

    let request = Request::build("textDocument/prepareCallHierarchy")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let items: Vec<CallHierarchyItem> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();
    let item = &items[0];

    // 2. Test Incoming Calls (should show TSX query)
    let params = CallHierarchyIncomingCallsParams {
        item: item.clone(),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let request = Request::build("callHierarchy/incomingCalls")
        .id(2)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let incoming: Option<Vec<CallHierarchyIncomingCall>> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let calls = incoming.expect("Expected incoming calls");
    assert!(calls.iter().any(|c| c.from.uri == tsx_uri));
}
