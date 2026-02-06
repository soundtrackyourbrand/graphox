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
async fn test_cross_project_docs_and_imports() {
    let dir = tempdir().unwrap();
    let base_dir = fs::canonicalize(dir.path()).unwrap();

    let p1_dir = base_dir.join("project1");
    let p2_dir = base_dir.join("project2");
    fs::create_dir(&p1_dir).unwrap();
    fs::create_dir(&p2_dir).unwrap();

    fs::write(p1_dir.join("package.json"), "{}").unwrap();
    fs::write(p2_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        r#"
        type Query {
            """
            This is a documented user field
            """
            user: User
        }
        "This is a documented User type"
        type User {
            id: ID!
            name: String
        }
    "#,
    )
    .unwrap();

    let config = Config {
        projects: vec![
            ProjectConfig {
                schema: SchemaSource::Single("schema.graphql".to_string()),
                include: GlobPattern::Single("project1/**/*.graphql".to_string()),
                exclude: None,
                output_dir: None,
                import: Some("@my/project1".to_string()),
                generate_permissions: None,
                codegen: Some(false),
            },
            ProjectConfig {
                schema: SchemaSource::Single("schema.graphql".to_string()),
                include: GlobPattern::Single("project2/**/*.graphql".to_string()),
                exclude: None,
                output_dir: None,
                import: Some("@my/project2".to_string()),
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

    // 1. Fragment in Project 1 with a comment
    let frag_path = p1_dir.join("fragments.graphql");
    let fragment_text = r#"
        # Documentation for UserFields
        fragment UserFields on User @public {
            id
            name
        }
    "#;
    fs::write(&frag_path, fragment_text).unwrap();
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
    let query_text = "query { user { ... } }";
    fs::write(&query_path, query_text).unwrap();
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

    // 3. Test Completion in Project 2
    let position = Position::new(0, 18); // "query { user { ...| } }"
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };

    let request = Request::build("textDocument/completion")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let items = match result.unwrap() {
        CompletionResponse::Array(items) => items,
        _ => panic!("Expected array"),
    };

    let item = items
        .iter()
        .find(|i| i.label == "UserFields")
        .expect("Should find UserFields");
    let doc = match item.documentation.as_ref().unwrap() {
        Documentation::MarkupContent(m) => &m.value,
        _ => panic!("Expected markup"),
    };

    assert!(doc.contains("Documentation for UserFields"));
    assert!(doc.contains("Import: `@my/project1`"));

    // 4. Test Hover in Project 2
    // First update the query to use the fragment
    let updated_text = "query { user { ...UserFields } }";
    service
        .call(
            Request::build("textDocument/didChange")
                .params(
                    serde_json::to_value(DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: query_uri.clone(),
                            version: 2,
                        },
                        content_changes: vec![TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: updated_text.to_string(),
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let position = Position::new(0, 22); // on "UserFields"
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
    };

    let request = Request::build("textDocument/hover")
        .id(2)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<Hover> = serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let hover = result.expect("Expected hover");
    let value = match hover.contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("Expected markup"),
    };

    assert!(value.contains("Documentation for UserFields"));
    assert!(value.contains("Import: `@my/project1`"));

    // 5. Test Field Completion with Documentation
    let query_with_space = "query {  }";
    service
        .call(
            Request::build("textDocument/didChange")
                .params(
                    serde_json::to_value(DidChangeTextDocumentParams {
                        text_document: VersionedTextDocumentIdentifier {
                            uri: query_uri.clone(),
                            version: 3,
                        },
                        content_changes: vec![TextDocumentContentChangeEvent {
                            range: None,
                            range_length: None,
                            text: query_with_space.to_string(),
                        }],
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let position = Position::new(0, 8); // "query { | }"
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };

    let request = Request::build("textDocument/completion")
        .id(3)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let items = match result.unwrap() {
        CompletionResponse::Array(items) => items,
        _ => panic!("Expected array"),
    };

    let item = items
        .iter()
        .find(|i| i.label == "user")
        .expect("Should find user");
    let doc = match item.documentation.as_ref().unwrap() {
        Documentation::MarkupContent(m) => &m.value,
        _ => panic!("Expected markup"),
    };

    assert!(doc.contains("This is a documented user field"));
}
