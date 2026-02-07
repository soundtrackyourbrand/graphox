use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    pos, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_goto_definition_cross_file() {
    let (dir, config) = make_temp_project_with_schema(
        "type Query { user: User } type User { id: ID! name: String }",
        "**/*.graphql",
    );
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Create and Open the fragment definition file
    let fragment_text = "fragment UserFields on User { id name }";
    let fragment_uri = write_project_file(&dir, "user_fragment.graphql", fragment_text);
    lsp_did_open(&mut service, fragment_uri.clone(), "graphql", 1, fragment_text).await;

    // 2. Create and Open the query file that uses the fragment
    let query_text = "query GetUser { user { ...UserFields } }";
    let query_uri = write_project_file(&dir, "query_with_fragment.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // 3. Trigger Go to Definition on "...UserFields" in query file
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri },
            position: pos(0, 26),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

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
    let (dir, config) = make_temp_project_with_schema(schema_text, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_uri = write_project_file(&dir, "schema.graphql", schema_text);
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, schema_text).await;

    // 1. From fragment type condition to type definition
    let frag_text = "fragment UserFields on Profile { bio }";
    let frag_uri = write_project_file(&dir, "frag.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    // Click on "Profile" in "fragment UserFields on Profile"
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: frag_uri.clone(),
            },
            position: pos(0, 25),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        assert_eq!(loc.range.start.line, 3);
        assert_eq!(loc.range.start.character, 5); // "type Profile"
    } else {
        panic!("Expected definition of Profile, got {:?}", result);
    }

    // 2. From field type to type definition (in schema)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: schema_uri.clone(),
            },
            position: pos(2, 45), // profile: Pro|file
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        assert_eq!(loc.range.start.line, 3);
    } else {
        panic!("Expected definition of Profile, got {:?}", result);
    }

    // 3. From variable type to type definition
    let query_text = "query ($input: MyInput) { user { id } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // Click on "MyInput" in "query ($input: MyInput)"
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: pos(0, 18), // My|Input
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        assert_eq!(loc.range.start.line, 1);
        assert_eq!(loc.range.start.character, 6); // "input MyInput"
    } else {
        panic!("Expected definition of MyInput, got {:?}", result);
    }

    // 4. Variable definition from usage
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: pos(0, 8), // $in|put
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, query_uri);
        assert_eq!(loc.range.start.line, 0);
        assert_eq!(loc.range.start.character, 7); // "$input"
    } else {
        panic!("Expected definition of $input, got {:?}", result);
    }

    // 5. Field definition from usage in fragment
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: frag_uri.clone(),
            },
            position: pos(0, 35), // bi|o
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        assert_eq!(loc.range.start.line, 3);
        assert_eq!(loc.range.start.character, 15); // "type Profile { bio"
    } else {
        panic!("Expected definition of field 'bio', got {:?}", result);
    }
}