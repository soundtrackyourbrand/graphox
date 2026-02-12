use crate::support::{
    create_doc, create_initialized_lsp_service, lsp_did_open, lsp_request_typed,
    make_temp_project_with_schema, range_for_token, with_cursor, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_cross_file() {
    let (dir, config) = make_temp_project_with_schema(
        "type Query { user: User } type User { id: ID! name: String }",
        "**/*.graphql",
    );
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Create and Open the fragment definition file
    let fragment_text = "fragment UserFields on User { id name }";
    let fragment_uri = write_project_file(&dir, "user_fragment.graphql", fragment_text);
    lsp_did_open(
        &mut service,
        fragment_uri.clone(),
        "graphql",
        1,
        fragment_text,
    )
    .await;

    // 2. Create and Open the query file that uses the fragment
    let query_text = "query GetUser { user { ...|UserFields } }";
    let (query_text, position) = with_cursor(query_text);
    let query_uri = write_project_file(&dir, "query_with_fragment.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    // 3. Trigger Go to Definition on "...UserFields" in query file
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    match result {
        Some(GotoDefinitionResponse::Scalar(location)) => {
            assert_eq!(location.uri, fragment_uri);
            let frag_doc = create_doc("file:///frag.graphql", fragment_text);
            assert_eq!(
                location.range,
                range_for_token(&frag_doc, fragment_text, "UserFields")
            );
        }
        _ => panic!("Expected Scalar location, got {:?}", result),
    }
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_types() {
    let schema_text = "scalar CustomScalar\ninput MyInput { id: ID }\ntype User { id: ID! name: String profile: Profile }\ntype Profile { bio: String }\ntype Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema_text, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_uri = write_project_file(&dir, "schema.graphql", schema_text);
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, schema_text).await;
    let schema_doc = create_doc(schema_uri.as_str(), schema_text);

    // 1. From fragment type condition to type definition
    let frag_text = "fragment UserFields on |Profile { bio }";
    let (frag_text, position) = with_cursor(frag_text);
    let frag_uri = write_project_file(&dir, "frag.graphql", &frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, &frag_text).await;

    // Click on "Profile" in "fragment UserFields on Profile"
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: frag_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        assert_eq!(
            loc.range,
            range_for_token(&schema_doc, schema_text, "Profile")
        );
    } else {
        panic!("Expected definition of Profile, got {:?}", result);
    }

    // 2. From field type to type definition (in schema)
    let (_schema_text_with_cursor, position) = with_cursor(
        "scalar CustomScalar\ninput MyInput { id: ID }\ntype User { id: ID! name: String profile: Pro|file }\ntype Profile { bio: String }\ntype Query { user: User }",
    );
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: schema_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        assert_eq!(
            loc.range,
            range_for_token(&schema_doc, schema_text, "Profile")
        );
    } else {
        panic!("Expected definition of Profile, got {:?}", result);
    }

    // 3. From variable type to type definition
    let query_text = "query ($input: My|Input) { user { id } }";
    let (query_text, position) = with_cursor(query_text);
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    // Click on "MyInput" in "query ($input: MyInput)"
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        assert_eq!(
            loc.range,
            range_for_token(&schema_doc, schema_text, "MyInput")
        );
    } else {
        panic!("Expected definition of MyInput, got {:?}", result);
    }

    // 4. Variable definition from usage
    let (query_text, position) = with_cursor("query ($in|put: MyInput) { user { id } }");
    let query_uri = write_project_file(&dir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;
    let query_doc = create_doc(query_uri.as_str(), &query_text);

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, query_uri);
        assert_eq!(
            loc.range,
            range_for_token(&query_doc, &query_text, "$input")
        );
    } else {
        panic!("Expected definition of $input, got {:?}", result);
    }

    // 5. Field definition from usage in fragment
    let frag_text = "fragment UserFields on Profile { bi|o }";
    let (frag_text, position) = with_cursor(frag_text);
    let frag_uri = write_project_file(&dir, "frag.graphql", &frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, &frag_text).await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: frag_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        assert_eq!(loc.range, range_for_token(&schema_doc, schema_text, "bio"));
    } else {
        panic!("Expected definition of field 'bio', got {:?}", result);
    }
}
