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
async fn test_goto_definition_type_vs_fragment_collision() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User }\ntype User { id: ID! }\ntype Displayable { id: ID! }",
    )
    .unwrap();
    let schema_path = std::fs::canonicalize(schema_path).unwrap();
    let schema_uri = Url::from_file_path(&schema_path).unwrap();

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
        lsp_automatic_codegen: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    let _ = service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await;
    let _ = service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await;

    // Open schema
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: schema_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: fs::read_to_string(&schema_path).unwrap(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Create a fragment where fragment name = type name
    let frag_text = "fragment Displayable on Displayable { id }";
    let frag_path = base_dir.join("frag.graphql");
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

    // 1. Trigger Go to Definition on "Displayable" in type condition position
    let response = service
        .call(
            Request::build("textDocument/definition")
                .id(1)
                .params(
                    serde_json::to_value(GotoDefinitionParams {
                        text_document_position_params: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier {
                                uri: frag_uri.clone(),
                            },
                            position: Position::new(0, 25),
                        },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();

    let result: Option<GotoDefinitionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        assert_eq!(loc.range.start.line, 2);
    } else {
        panic!("Expected definition of Displayable type, got {:?}", result);
    }

    // 2. Trigger Go to Definition on fragment spread
    let query_text = "query GetUser { user { ...Displayable } }";
    let query_path = base_dir.join("query.graphql");
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

    let response = service
        .call(
            Request::build("textDocument/definition")
                .id(2)
                .params(
                    serde_json::to_value(GotoDefinitionParams {
                        text_document_position_params: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier {
                                uri: query_uri.clone(),
                            },
                            position: Position::new(0, 26),
                        },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();

    let result: Option<GotoDefinitionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, frag_uri);
        assert_eq!(loc.range.start.line, 0);
        assert_eq!(loc.range.start.character, 9);
    } else {
        panic!(
            "Expected definition of Displayable fragment, got {:?}",
            result
        );
    }
}

#[tokio::test]
async fn test_goto_definition_directive() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "directive @customDirective(arg: String) on FIELD\ntype Query { id: ID }",
    )
    .unwrap();
    let schema_path = std::fs::canonicalize(schema_path).unwrap();
    let schema_uri = Url::from_file_path(&schema_path).unwrap();

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
        lsp_automatic_codegen: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    let _ = service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await;
    let _ = service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await;

    // Open schema
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: schema_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: fs::read_to_string(&schema_path).unwrap(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let query_text = "query { id @customDirective(arg: \"test\") }";
    let query_path = base_dir.join("query.graphql");
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

    let response = service
        .call(
            Request::build("textDocument/definition")
                .id(1)
                .params(
                    serde_json::to_value(GotoDefinitionParams {
                        text_document_position_params: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier {
                                uri: query_uri.clone(),
                            },
                            position: Position::new(0, 15),
                        },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();

    let result: Option<GotoDefinitionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        assert_eq!(loc.range.start.line, 0);
        assert_eq!(loc.range.start.character, 11); // directive @|customDirective
    } else {
        panic!("Expected definition of customDirective, got {:?}", result);
    }
}

#[tokio::test]
async fn test_goto_definition_variable_in_argument() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user(id: ID!): User }\ntype User { id: ID! name: String }",
    )
    .unwrap();
    let schema_path = std::fs::canonicalize(schema_path).unwrap();
    let schema_uri = Url::from_file_path(&schema_path).unwrap();

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
        lsp_automatic_codegen: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    let _ = service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await;
    let _ = service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await;

    // Open schema
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: schema_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: fs::read_to_string(&schema_path).unwrap(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    let query_text = "query GetUser($id: ID!) { user(id: $id) { name } }";
    let query_path = base_dir.join("query.graphql");
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

    // 1. Trigger Go to Definition on "$id" in "id: $id"
    let response = service
        .call(
            Request::build("textDocument/definition")
                .id(1)
                .params(
                    serde_json::to_value(GotoDefinitionParams {
                        text_document_position_params: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier {
                                uri: query_uri.clone(),
                            },
                            position: Position::new(0, 36), // On 'i' of $id in "id: $id"
                        },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();

    let result: Option<GotoDefinitionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, query_uri);
        assert_eq!(loc.range.start.line, 0);
        assert_eq!(loc.range.start.character, 14); // query GetUser(|$id...
    } else {
        panic!("Expected definition of $id variable, got {:?}", result);
    }

    // 2. Trigger Go to Definition on "id" (argument name) in "user(id: $id)"
    let response = service
        .call(
            Request::build("textDocument/definition")
                .id(2)
                .params(
                    serde_json::to_value(GotoDefinitionParams {
                        text_document_position_params: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier {
                                uri: query_uri.clone(),
                            },
                            position: Position::new(0, 31),
                        },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();

    let result: Option<GotoDefinitionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        assert_eq!(loc.range.start.line, 0);
        assert_eq!(loc.range.start.character, 18); // user(|id: ID!)
    } else {
        panic!(
            "Expected definition of id argument in schema, got {:?}",
            result
        );
    }
}
