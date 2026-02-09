use crate::support::{
    completion_items_array, create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    make_temp_project_with_schema, with_cursor, write_project_file,
};

#[tokio::test]
async fn test_completion_unclosed_variable() {
    let schema = "type Query { user(id: ID!): User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query GetUser($userId: ID!) { user(id: $| }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
    assert!(
        items.iter().any(|i| i.label == "$userId"),
        "Expected '$userId' in completions: {:?}",
        labels
    );
}

#[tokio::test]
async fn test_completion_variables() {
    let schema = "type Query { user(id: ID!): User } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query GetUser($userId: ID!) { user(id: $|) }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
    assert!(
        items.iter().any(|i| i.label == "$userId"),
        "Expected $userId in completions: {:?}",
        labels
    );
}
