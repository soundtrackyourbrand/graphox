use crate::support::{
    completion_items_array, create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    make_temp_project_with_schema, with_cursor, write_project_file,
};

#[tokio::test]
#[ntest::timeout(3000)]
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
#[ntest::timeout(3000)]
async fn test_completion_variables() {
    let schema = "type Query { user(id: ID!): User } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config = config.with_base_dir(dir.path().to_path_buf());

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

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_variable_default_value() {
    let schema = r#"
      enum Status { ACTIVE, INACTIVE }
      type Query {
        user(status: Status, isAdmin: Boolean): User
      }
      type User { id: ID! }
    "#;
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) =
        with_cursor("query ($status: Status=|) { user(status: $status) { id } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"ACTIVE"));
    assert!(labels.contains(&"INACTIVE"));
    assert!(labels.contains(&"null"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_variable_default_value_boolean() {
    let schema = r#"
      type Query {
        user(isAdmin: Boolean): User
      }
      type User { id: ID! }
    "#;
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) =
        with_cursor("query ($isAdmin: Boolean = |) { user(isAdmin: $isAdmin) { id } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"true"));
    assert!(labels.contains(&"false"));
    assert!(labels.contains(&"null"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_list_default_value() {
    let schema = r#"
      type Query {
        users(ids: [ID]): [User]
      }
      type User { id: ID! }
    "#;
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query ($ids: [ID] = |) { users(ids: $ids) { id } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"[]"));
    assert!(labels.contains(&"null"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_non_null_no_null_suggestion() {
    let schema = r#"
      enum Status { ACTIVE, INACTIVE }
      type Query { user: User }
      type User { id: ID! }
    "#;
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query ($status: Status! = |) { user { id } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"ACTIVE"));
    assert!(labels.contains(&"INACTIVE"));
    assert!(!labels.contains(&"null"));
}
