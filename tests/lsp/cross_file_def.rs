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
async fn test_goto_definition_cross_file() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    // Create package.json to define a package root
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
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // 0. Initialize
    let init_params = InitializeParams {
        ..Default::default()
    };
    let request = Request::build("initialize")
        .params(serde_json::to_value(&init_params).unwrap())
        .id(0)
        .finish();
    service.call(request).await.unwrap().unwrap();

    let request = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    service.call(request).await.unwrap();

    // 1. Create and Open the fragment definition file
    let frag_path = base_dir.join("user_fragment.graphql");
    let fragment_text = "fragment UserFields on User { id name }";
    fs::write(&frag_path, fragment_text).unwrap();
    let frag_path = std::fs::canonicalize(frag_path).unwrap();
    let fragment_uri = Url::from_file_path(&frag_path).unwrap();

    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: fragment_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: fragment_text.to_string(),
        },
    };
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(serde_json::to_value(&params).unwrap())
                .finish(),
        )
        .await
        .unwrap();

    // 2. Create and Open the query file that uses the fragment
    let query_path = base_dir.join("query_with_fragment.graphql");
    let query_text = "query GetUser { user { ...UserFields } }";
    fs::write(&query_path, query_text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let query_uri = Url::from_file_path(&query_path).unwrap();

    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: query_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: query_text.to_string(),
        },
    };
    service
        .call(
            Request::build("textDocument/didOpen")
                .params(serde_json::to_value(&params).unwrap())
                .finish(),
        )
        .await
        .unwrap();

    // 3. Trigger Go to Definition on "...UserFields" in query file
    let position = Position::new(0, 26);
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let request = Request::build("textDocument/definition")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = service.call(request).await.unwrap().unwrap();
    if let Some(err) = response.error() {
        panic!("LSP Error: {:?}", err);
    }
    let result: Option<GotoDefinitionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, fragment_uri);
            assert_eq!(location.range.start.line, 0);
            assert_eq!(location.range.start.character, 9);
        }
        _ => panic!("Expected Scalar location, got {:?}", result),
    }
}

#[tokio::test]
async fn test_goto_definition_types() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "scalar CustomScalar\ninput MyInput { id: ID }\ntype User { id: ID! name: String profile: Profile }\ntype Profile { bio: String }\ntype Query { user: User }",
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
        enable_schema_cache: Some(true),
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

    // 1. From fragment type condition to type definition
    let frag_text = "fragment UserFields on Profile { bio }";
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

    // Click on "Profile" in "fragment UserFields on Profile"
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
        assert_eq!(loc.range.start.line, 3);
        assert_eq!(loc.range.start.character, 5); // "type Profile"
    } else {
        panic!("Expected definition of Profile, got {:?}", result);
    }

    // 2. From field type to type definition (in schema)
    // Click on "Profile" in "type User { ... profile: Profile }"
    let response = service
        .call(
            Request::build("textDocument/definition")
                .id(2)
                .params(
                    serde_json::to_value(GotoDefinitionParams {
                        text_document_position_params: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier {
                                uri: schema_uri.clone(),
                            },
                            position: Position::new(2, 45), // profile: Pro|file
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
        assert_eq!(loc.range.start.line, 3);
    } else {
        panic!("Expected definition of Profile, got {:?}", result);
    }

    // 3. From variable type to type definition
    let query_text = "query ($input: MyInput) { user { id } }";
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

    // Click on "MyInput" in "query ($input: MyInput)"
    let response = service
        .call(
            Request::build("textDocument/definition")
                .id(3)
                .params(
                    serde_json::to_value(GotoDefinitionParams {
                        text_document_position_params: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier {
                                uri: query_uri.clone(),
                            },
                            position: Position::new(0, 18), // My|Input
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
        assert_eq!(loc.range.start.line, 1);
        assert_eq!(loc.range.start.character, 6); // "input MyInput"
    } else {
        panic!("Expected definition of MyInput, got {:?}", result);
    }

    // 4. Variable definition from usage
    // Click on "$input" in "query ($input: MyInput)" or later usage (not here but let's test definition itself)
    let response = service
        .call(
            Request::build("textDocument/definition")
                .id(4)
                .params(
                    serde_json::to_value(GotoDefinitionParams {
                        text_document_position_params: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier {
                                uri: query_uri.clone(),
                            },
                            position: Position::new(0, 8), // $in|put
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
        assert_eq!(loc.range.start.character, 7); // "$input"
    } else {
        panic!("Expected definition of $input, got {:?}", result);
    }

    // 5. Field definition from usage in fragment
    // Click on "bio" in "fragment UserFields on Profile { bio }"
    let response = service
        .call(
            Request::build("textDocument/definition")
                .id(5)
                .params(
                    serde_json::to_value(GotoDefinitionParams {
                        text_document_position_params: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier {
                                uri: frag_uri.clone(),
                            },
                            position: Position::new(0, 35), // bi|o
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
        assert_eq!(loc.range.start.line, 3);
        assert_eq!(loc.range.start.character, 15); // "type Profile { bio"
    } else {
        panic!("Expected definition of field 'bio', got {:?}", result);
    }
}
