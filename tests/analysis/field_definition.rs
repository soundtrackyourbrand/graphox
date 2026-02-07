use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    pos, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_goto_definition_fields_and_extensions() {
    let schema_text = r#"
        type Query {
            me: User!
        }
        type User {
            id: ID!
        }
        extend type User {
            username: String!
        }
        "#;
    let (dir, config) = make_temp_project_with_schema(schema_text, "**/*.{graphql,tsx}");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // NOTE: We are NOT opening the schema.graphql file here.
    // It should be loaded via workspace scan or as a project schema.

    let query_text = r#"
        query {
            me {
                id
                username
            }
        }
    "#;
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // 1. Go to definition for 'id' (regular field)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: pos(3, 17),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    assert!(result.is_some(), "Should find definition for 'id'");

    // 2. Go to definition for 'username' (extended field)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: pos(4, 17),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    assert!(
        result.is_some(),
        "Should find definition for 'username' in extension"
    );
}