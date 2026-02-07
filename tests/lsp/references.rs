use graphql_rust::{
    Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
};
use std::fs;
use tempfile::tempdir;
use crate::support;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test]
async fn test_fragment_references() {
    let (dir, config) = support::make_temp_project_with_schema(
        "type Query { user: User } type User { id: ID! name: String }",
        "**/*.graphql",
    );
    let (mut service, _handle) = support::create_initialized_lsp_service(config).await;

    // 1. Create and Open the fragment definition file
    let fragment_text = "fragment UserFields on User { id name }";
    let fragment_uri = support::write_project_file(&dir, "user_fragment.graphql", fragment_text);
    support::lsp_did_open(&mut service, fragment_uri.clone(), "graphql", 1, fragment_text).await;

    // 2. Create and Open the query file that uses the fragment
    let query_text = "query GetUser { user { ...UserFields } }";
    let query_uri = support::write_project_file(&dir, "query_with_fragment.graphql", query_text);
    support::lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // 3. Trigger Find References on "UserFields" in fragment file
    let position = Position::new(0, 9);
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

    let locations = result.expect("Expected locations");
    assert_eq!(locations.len(), 2); // 1 definition + 1 spread

    let has_def = locations
        .iter()
        .any(|l| l.uri == fragment_uri && l.range.start.character == 9);
    let has_spread = locations
        .iter()
        .any(|l| l.uri == query_uri && l.range.start.character == 26);

    assert!(has_def, "Missing definition in references");
    assert!(has_spread, "Missing spread in references");
}

#[tokio::test]
async fn test_fragment_references_tsx() {
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
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _handle) = support::create_initialized_lsp_service(config).await;

    // 1. Fragment in .graphql file
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

    // 2. Usage in .tsx file
    let tsx_path = base_dir.join("Component.tsx");
    let tsx_text = r#"
        import { gql } from './gql';
        const query = gql`
            query GetUser {
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

    // 3. Trigger Find References on "UserFields" in fragment file
    let position = Position::new(0, 9);
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

    let locations = result.expect("Expected locations");
    assert_eq!(locations.len(), 2);

    let has_tsx_spread = locations.iter().any(|l| {
        l.uri == tsx_uri
            && tsx_text
                .lines()
                .nth(l.range.start.line as usize)
                .unwrap()
                .contains("...UserFields")
    });
    assert!(has_tsx_spread, "Missing TSX spread in references");
}

#[tokio::test]
async fn test_fragment_references_exclude_declaration() {
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
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = crate::support::create_service(config);

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

    // 3. Trigger Find References with include_declaration: false
    let position = Position::new(0, 9);
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
            include_declaration: false,
        },
    };

    let request = Request::build("textDocument/references")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<Vec<Location>> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let locations = result.expect("Expected locations");
    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].uri, query_uri);
}
