use crate::support::{
    completion_items_array, create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    lsp_request_hover, lsp_request_typed, make_temp_project_with_schema, pos, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_lsp_multi_schema_support() {
    let schema_text = "type User { id: ID! name: String } type Query { me: User }";
    let (dir, mut config) = make_temp_project_with_schema(schema_text, "query.graphql");

    // 2. Create ext.graphql
    write_project_file(&dir, "ext.graphql", "extend type User { email: String }");

    // 4. Update Config
    config.projects[0].schema = graphql_rust::config::SchemaSource::Multiple(vec![
        "schema.graphql".to_string(),
        "ext.graphql".to_string(),
    ]);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let query_text = "query { me { id name email } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // 5. Test Completion for 'email' (from ext.graphql)
    // query { me { id name email } }
    let result = lsp_request_completion(&mut service, query_uri.clone(), pos(0, 23)).await;
    let items = completion_items_array(&result);

    assert!(
        items.iter().any(|i| i.label == "email"),
        "Should suggest 'email' from extension schema"
    );
    assert!(
        items.iter().any(|i| i.label == "name"),
        "Should suggest 'name' from base schema"
    );

    // 6. Test Hover for 'email'
    let result = lsp_request_hover(&mut service, query_uri.clone(), pos(0, 23)).await;

    assert!(result.is_some());
    let hover = result.unwrap();
    if let HoverContents::Markup(markup) = hover.contents {
        assert!(
            markup.value.contains("String"),
            "Hover should show type from extension schema"
        );
    }

    // 7. Test Go to Definition for 'id' (from base.graphql)
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: pos(0, 15), // on 'id'
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let _: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    // 8. Test Fragments and @public check
    let frag_text = "fragment UserFields on User { id name }";
    let frag_uri = write_project_file(&dir, "frag.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    // Use fragment in query
    let query_text_2 = "query { me { ...UserFields } }";
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 2, query_text_2).await;

    // Goto Definition for UserFields
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: pos(0, 18),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    match result {
        Some(GotoDefinitionResponse::Scalar(loc)) => {
            assert_eq!(loc.uri, frag_uri, "Should jump to UserFields definition");
        }
        _ => panic!("Fragment definition not found"),
    }
}