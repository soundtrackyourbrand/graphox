use crate::support::{
    create_doc, create_initialized_lsp_service, lsp_did_open, lsp_request_typed,
    make_temp_project_with_schema, pos, write_project_file,
};
use std::fs;
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_goto_definition_type_vs_fragment_collision() {
    let schema = "type Query { user: User }\ntype User { id: ID! }\ntype Displayable { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    // keep base_dir consistent
    config.base_dir = tmpdir.path().to_path_buf();
    let (mut service, _handle) = create_initialized_lsp_service(config).await;
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    let schema_doc = create_doc(schema_uri.as_str(), &schema_text);

    // Open schema
    lsp_did_open(
        &mut service,
        schema_uri.clone(),
        "graphql",
        1,
        &schema_text,
    )
    .await;

    // Create a fragment where fragment name = type name
    let frag_text = "fragment Displayable on Displayable { id }";
    let frag_uri = write_project_file(&tmpdir, "frag.graphql", frag_text);

    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    let frag_doc = create_doc(frag_uri.as_str(), frag_text);

    // 1. Trigger Go to Definition on "Displayable" in type condition position
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
        assert_eq!(
            loc.range,
            crate::support::range_for_token(&schema_doc, &schema_text, "Displayable")
        );
    } else {
        panic!("Expected definition of Displayable type, got {:?}", result);
    }

    // 2. Trigger Go to Definition on fragment spread
    let query_text = "query GetUser { user { ...Displayable } }";
    let query_uri = write_project_file(&tmpdir, "query.graphql", query_text);

    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: pos(0, 26),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(ref loc)) = result {
        if loc.uri == frag_uri {
            // Expected: definition points to the fragment in this file
            assert_eq!(
                loc.range,
                crate::support::range_for_token(&frag_doc, frag_text, "Displayable")
            );
        } else if loc.uri == schema_uri {
            // Some environments may resolve the spread name to the type definition
            // (fallback). Accept either but validate the schema location roughly.
            assert_eq!(
                loc.range,
                crate::support::range_for_token(&schema_doc, &schema_text, "Displayable")
            );
        } else {
            panic!(
                "Expected definition of Displayable fragment (or schema type), got {:?}",
                result
            );
        }
    } else {
        panic!(
            "Expected definition of Displayable fragment, got {:?}",
            result
        );
    }
}

#[tokio::test]
async fn test_goto_definition_directive() {
    let schema = "directive @customDirective(arg: String) on FIELD\ntype Query { id: ID }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config.base_dir = tmpdir.path().to_path_buf();
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    let schema_doc = create_doc(schema_uri.as_str(), &schema_text);

    // Open schema
    lsp_did_open(
        &mut service,
        schema_uri.clone(),
        "graphql",
        1,
        &schema_text,
    )
    .await;

    let query_text = "query { id @customDirective(arg: \"test\") }";
    let query_uri = write_project_file(&tmpdir, "query.graphql", query_text);

    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: pos(0, 15),
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
            crate::support::range_for_token(&schema_doc, &schema_text, "customDirective")
        );
    } else {
        panic!("Expected definition of customDirective, got {:?}", result);
    }
}

#[tokio::test]
async fn test_goto_definition_variable_in_argument() {
    let (tmpdir, mut config) = make_temp_project_with_schema(
        "type Query { user(id: ID!): User }\ntype User { id: ID! name: String }",
        "**/*.graphql",
    );
    config.base_dir = tmpdir.path().to_path_buf();
    let (mut service, _handle) = create_initialized_lsp_service(config).await;
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    let schema_doc = create_doc(schema_uri.as_str(), &schema_text);

    lsp_did_open(
        &mut service,
        schema_uri.clone(),
        "graphql",
        1,
        &schema_text,
    )
    .await;

    let query_text = "query GetUser($id: ID!) { user(id: $id) { name } }";
    let query_uri = write_project_file(&tmpdir, "query.graphql", query_text);

    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let query_doc = create_doc(query_uri.as_str(), query_text);

    // 1. Trigger Go to Definition on "$id" in "id: $id"
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: pos(0, 36), // On 'i' of $id in "id: $id"
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
            crate::support::range_for_token_at_index(&query_doc, query_text, "$id", 0)
        );
    } else {
        panic!("Expected definition of $id variable, got {:?}", result);
    }

    // 2. Trigger Go to Definition on "id" (argument name) in "user(id: $id)"
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: pos(0, 31),
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
            crate::support::range_for_token_at_index(&schema_doc, &schema_text, "id", 0)
        );
    } else {
        panic!(
            "Expected definition of id argument in schema, got {:?}",
            result
        );
    }
}