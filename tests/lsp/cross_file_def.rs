use std::fs;
use crate::support;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test]
async fn test_goto_definition_cross_file() {
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
    let schema_text = "scalar CustomScalar\ninput MyInput { id: ID }\ntype User { id: ID! name: String profile: Profile }\ntype Profile { bio: String }\ntype Query { user: User }";
    let (dir, config) = support::make_temp_project_with_schema(schema_text, "**/*.graphql");
    let (mut service, _handle) = support::create_initialized_lsp_service(config).await;

    // Open schema
    let schema_uri = support::write_project_file(&dir, "schema.graphql", schema_text);
    support::lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, schema_text).await;

    // 1. From fragment type condition to type definition
    let frag_text = "fragment UserFields on Profile { bio }";
    let frag_path = dir.path().join("frag.graphql");
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
    let query_path = dir.path().join("query.graphql");
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
