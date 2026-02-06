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
async fn test_cross_project_references_and_rename() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    let p1_dir = base_dir.join("project1");
    let p2_dir = base_dir.join("project2");
    fs::create_dir(&p1_dir).unwrap();
    fs::create_dir(&p2_dir).unwrap();

    fs::write(p1_dir.join("package.json"), "{}").unwrap();
    fs::write(p2_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User } type User { id: ID! name: String }",
    )
    .unwrap();

    let config = Config {
        projects: vec![
            ProjectConfig {
                schema: SchemaSource::Single("../schema.graphql".to_string()),
                include: GlobPattern::Single("**/*.graphql".to_string()),
                exclude: None,
                output_dir: None,
                import: None,
                generate_permissions: None,
                codegen: Some(false),
            },
            ProjectConfig {
                schema: SchemaSource::Single("../schema.graphql".to_string()),
                include: GlobPattern::Single("**/*.graphql".to_string()),
                exclude: None,
                output_dir: None,
                import: None,
                generate_permissions: None,
                codegen: Some(false),
            },
        ],
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        timeouts: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
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

    // 1. Fragment in Project 1 (public)
    let frag_path = p1_dir.join("fragments.graphql");
    let fragment_text = "fragment UserFields on User @public { id name }";
    fs::write(&frag_path, fragment_text).unwrap();
    let frag_path = std::fs::canonicalize(frag_path).unwrap();
    let fragment_uri = Url::from_file_path(&frag_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
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

    // 2. Usage in Project 2
    let query_path = p2_dir.join("query.graphql");
    let query_text = "query { user { ...UserFields } }";
    fs::write(&query_path, query_text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let query_uri = Url::from_file_path(&query_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
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

    // 3. Test References from Project 1 to Project 2
    let position = Position::new(0, 9); // on "UserFields" in project1
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: fragment_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let request = Request::build("textDocument/references")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<Vec<Location>> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let locations = result.expect("Expected references");
    assert_eq!(locations.len(), 2);
    assert!(locations.iter().any(|l| l.uri == fragment_uri));
    assert!(locations.iter().any(|l| l.uri == query_uri));

    // 4. Test Rename from Project 1 affecting Project 2
    let rename_params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: fragment_uri.clone(),
            },
            position,
        },
        new_name: "SharedFields".to_string(),
        work_done_progress_params: Default::default(),
    };

    let request = Request::build("textDocument/rename")
        .id(2)
        .params(serde_json::to_value(&rename_params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<WorkspaceEdit> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let edit = result.expect("Expected WorkspaceEdit");
    let changes = edit.changes.expect("Expected changes");

    assert_eq!(changes.len(), 2);
    assert!(changes.contains_key(&fragment_uri));
    assert!(changes.contains_key(&query_uri));

    assert_eq!(changes[&query_uri][0].new_text, "SharedFields");
}

#[tokio::test]
async fn test_unrelated_projects_rename_isolation() {
    // This test documents that CURRENTLY rename is workspace-wide and NOT isolated by project/package
    // if the fragment names match.
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    let p1_dir = base_dir.join("project1");
    let p2_dir = base_dir.join("project2");
    fs::create_dir(&p1_dir).unwrap();
    fs::create_dir(&p2_dir).unwrap();

    fs::write(p1_dir.join("package.json"), "{}").unwrap();
    fs::write(p2_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User } type User { id: ID! name: String }",
    )
    .unwrap();

    let config = Config {
        projects: vec![
            ProjectConfig {
                schema: SchemaSource::Single("../schema.graphql".to_string()),
                include: GlobPattern::Single("project1/**/*.graphql".to_string()),
                exclude: None,
                output_dir: None,
                import: None,
                generate_permissions: None,
                codegen: Some(false),
            },
            ProjectConfig {
                schema: SchemaSource::Single("../schema.graphql".to_string()),
                include: GlobPattern::Single("project2/**/*.graphql".to_string()),
                exclude: None,
                output_dir: None,
                import: None,
                generate_permissions: None,
                codegen: Some(false),
            },
        ],
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        timeouts: None,
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

    // Project 1: Private fragment
    let p1_frag = p1_dir.join("f1.graphql");
    fs::write(&p1_frag, "fragment LocalFields on User { id }").unwrap();
    let p1_frag = std::fs::canonicalize(p1_frag).unwrap();
    let p1_uri = Url::from_file_path(&p1_frag).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: p1_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: "fragment LocalFields on User { id }".to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Project 2: Also has a fragment with the same name (coincidental)
    let p2_frag = p2_dir.join("f2.graphql");
    fs::write(&p2_frag, "fragment LocalFields on User { name }").unwrap();
    let p2_frag = std::fs::canonicalize(p2_frag).unwrap();
    let p2_uri = Url::from_file_path(&p2_frag).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: p2_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: "fragment LocalFields on User { name }".to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Rename in Project 1
    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: p1_uri.clone(),
            },
            position: Position::new(0, 9),
        },
        new_name: "Project1Fields".to_string(),
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

    // CURRENT BEHAVIOR: It renames BOTH because it's workspace-wide by name
    assert!(changes.contains_key(&p1_uri));
    assert!(changes.contains_key(&p2_uri));
}
