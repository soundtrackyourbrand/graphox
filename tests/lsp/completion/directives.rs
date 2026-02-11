use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    make_temp_project_with_schema, with_cursor, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_completion_directives_on_field() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! } directive @testDirective on FIELD";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config.base_dir = dir.path().to_path_buf();
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { users { id @| } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;

    if let Some(CompletionResponse::Array(items)) = result {
        assert!(items.iter().any(|i| i.label == "testDirective"));
    } else {
        panic!("Expected array of completions");
    }
}

#[tokio::test]
async fn test_completion_directives_on_fragment() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config.base_dir = dir.path().to_path_buf();
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("fragment MyFrag on User @| { id }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;

    if let Some(CompletionResponse::Array(items)) = result {
        assert!(items.iter().any(|i| i.label == "public"));
        assert!(items.iter().any(|i| i.label == "type_only"));
    } else {
        panic!("Expected array of completions");
    }
}
