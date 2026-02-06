use graphql_rust::{
    Backend, Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
};
use std::fs;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test]
async fn test_fragment_rename() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User } type User { id: ID! name: String }",
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
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    let init_params = InitializeParams {
        ..Default::default()
    };
    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(&init_params).unwrap())
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

    // 1. Fragment file
    let frag_path = base_dir.join("user_fragment.graphql");
    let fragment_text = "fragment UserFields on User { id name }";
    fs::write(&frag_path, fragment_text).unwrap();
    let frag_path = std::fs::canonicalize(frag_path).unwrap();
    let fragment_uri = Url::from_file_path(&frag_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(&DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: fragment_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: fragment_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // 2. Query file
    let query_path = base_dir.join("query.graphql");
    let query_text = "query { user { ...UserFields } }";
    fs::write(&query_path, query_text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let query_uri = Url::from_file_path(&query_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(&DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: query_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: query_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // 3. Trigger Rename on "UserFields" in fragment file to "MyFields"
    let position = Position::new(0, 9);
    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: fragment_uri.clone(),
            },
            position,
        },
        new_name: "MyFields".to_string(),
        work_done_progress_params: Default::default(),
    };

    let request = Request::build("textDocument/rename")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<WorkspaceEdit> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let edit = result.expect("Expected WorkspaceEdit");
    let changes = edit.changes.expect("Expected changes");

    assert_eq!(changes.len(), 2);

    let frag_edits = &changes[&fragment_uri];
    assert_eq!(frag_edits.len(), 1);
    assert_eq!(frag_edits[0].new_text, "MyFields");
    assert_eq!(frag_edits[0].range.start.character, 9);

    let query_edits = &changes[&query_uri];
    assert_eq!(query_edits.len(), 1);
    assert_eq!(query_edits[0].new_text, "MyFields");
    assert_eq!(query_edits[0].range.start.character, 18);
}

#[tokio::test]
async fn test_fragment_rename_tsx() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User } type User { id: ID! name: String }",
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
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    let init_params = InitializeParams {
        ..Default::default()
    };
    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(&init_params).unwrap())
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

    // 1. Fragment file
    let frag_path = base_dir.join("user_fragment.graphql");
    let fragment_text = "fragment UserFields on User { id name }";
    fs::write(&frag_path, fragment_text).unwrap();
    let frag_path = std::fs::canonicalize(frag_path).unwrap();
    let fragment_uri = Url::from_file_path(&frag_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(&DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: fragment_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: fragment_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // 2. TSX file
    let tsx_path = base_dir.join("Component.tsx");
    let tsx_text = r#"
        const query = gql`
            query {
                user {
                    ...UserFields
                }
            }
        `;
    "#;
    fs::write(&tsx_path, tsx_text).unwrap();
    let tsx_path = std::fs::canonicalize(tsx_path).unwrap();
    let tsx_uri = Url::from_file_path(&tsx_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(&DidOpenTextDocumentParams {
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

    // 3. Trigger Rename on "UserFields" in fragment file
    let position = Position::new(0, 9);
    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: fragment_uri.clone(),
            },
            position,
        },
        new_name: "RenamedFields".to_string(),
        work_done_progress_params: Default::default(),
    };

    let request = Request::build("textDocument/rename")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<WorkspaceEdit> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let edit = result.expect("Expected WorkspaceEdit");
    let changes = edit.changes.expect("Expected changes");

    assert_eq!(changes.len(), 2);

    let tsx_edits = &changes[&tsx_uri];
    assert_eq!(tsx_edits.len(), 1);
    assert_eq!(tsx_edits[0].new_text, "RenamedFields");

    // Verify it correctly identified the location in TSX
    let line = tsx_text
        .lines()
        .nth(tsx_edits[0].range.start.line as usize)
        .unwrap();
    assert!(line.contains("...UserFields"));
}

#[tokio::test]
async fn test_rename_unopened_file() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User } type User { id: ID! name: String }",
    )
    .unwrap();

    // 1. Fragment file (will be opened)
    let frag_path = base_dir.join("user_fragment.graphql");
    let fragment_text = "fragment UserFields on User { id name }";
    fs::write(&frag_path, fragment_text).unwrap();
    let frag_path = std::fs::canonicalize(frag_path).unwrap();
    let fragment_uri = Url::from_file_path(&frag_path).unwrap();

    // 2. Query file (will NOT be opened)
    let query_path = base_dir.join("query.graphql");
    let query_text = "query { user { ...UserFields } }";
    fs::write(&query_path, query_text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let query_uri = Url::from_file_path(&query_path).unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    let init_params = InitializeParams {
        ..Default::default()
    };
    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(&init_params).unwrap())
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

    // Wait for workspace scan to complete
    let backend = service.inner();
    let mut attempts = 0;
    while !backend.workspace_loaded.load(Ordering::SeqCst) && attempts < 100 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        attempts += 1;
    }
    assert!(
        backend.workspace_loaded.load(Ordering::SeqCst),
        "Workspace scan timed out"
    );

    // Open only the fragment file
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(&DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: fragment_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: fragment_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Trigger Rename on "UserFields" in fragment file to "MyFields"
    let position = Position::new(0, 9);
    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: fragment_uri.clone(),
            },
            position,
        },
        new_name: "MyFields".to_string(),
        work_done_progress_params: Default::default(),
    };

    let request = Request::build("textDocument/rename")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<WorkspaceEdit> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let edit = result.expect("Expected WorkspaceEdit");
    let changes = edit.changes.expect("Expected changes");

    // Check if both files are present in the changes
    assert!(
        changes.contains_key(&fragment_uri),
        "Changes should include fragment file"
    );
    assert!(
        changes.contains_key(&query_uri),
        "Changes should include unopened query file"
    );

    let frag_edits = &changes[&fragment_uri];
    assert_eq!(frag_edits.len(), 1);
    assert_eq!(frag_edits[0].new_text, "MyFields");

    let query_edits = &changes[&query_uri];
    assert_eq!(query_edits.len(), 1);
    assert_eq!(query_edits[0].new_text, "MyFields");
    assert_eq!(query_edits[0].range.start.character, 18);
}
