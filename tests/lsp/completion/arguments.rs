use crate::support::{
    completion_items_array, create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    make_temp_project_with_schema, with_cursor, write_project_file,
};

#[tokio::test]
async fn test_completion_unclosed_arguments() {
    let schema = "type Query { user(id: ID!, name: String): User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { user(| }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
    assert!(
        items.iter().any(|i| i.label == "id"),
        "Expected 'id' in completions: {:?}",
        labels
    );
    assert!(
        items.iter().any(|i| i.label == "name"),
        "Expected 'name' in completions: {:?}",
        labels
    );
}

#[tokio::test]
async fn test_completion_unclosed_input_object() {
    let schema = "input CreateUserInput { username: String!, email: String } type Mutation { createUser(input: CreateUserInput!): User } type User { id: ID! } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("mutation { createUser(input: {| }) }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
    assert!(
        items.iter().any(|i| i.label == "username"),
        "Expected 'username' in completions: {:?}",
        labels
    );
    assert!(
        items.iter().any(|i| i.label == "email"),
        "Expected 'email' in completions: {:?}",
        labels
    );
}

#[tokio::test]
async fn test_completion_argument_names() {
    let schema = "type Query { user(id: ID!, name: String): User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { user(| ) }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
    assert!(
        items.iter().any(|i| i.label == "id"),
        "Expected 'id' in completions: {:?}",
        labels
    );
    assert!(
        items.iter().any(|i| i.label == "name"),
        "Expected 'name' in completions: {:?}",
        labels
    );
}

#[tokio::test]
async fn test_completion_input_object_fields() {
    let schema = "input CreateUserInput { username: String!, email: String } type Mutation { createUser(input: CreateUserInput!): User } type User { id: ID! } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("mutation { createUser(input: { | }) }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
    assert!(
        items.iter().any(|i| i.label == "username"),
        "Expected 'username' in completions: {:?}",
        labels
    );
    assert!(
        items.iter().any(|i| i.label == "email"),
        "Expected 'email' in completions: {:?}",
        labels
    );
}

#[tokio::test]
async fn test_completion_enum_values() {
    let schema = "enum Role { ADMIN, USER } type Query { users(role: Role): [String] }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { users(role: |) }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
    assert!(
        items.iter().any(|i| i.label == "ADMIN"),
        "Expected 'ADMIN' in completions: {:?}",
        labels
    );
    assert!(
        items.iter().any(|i| i.label == "USER"),
        "Expected 'USER' in completions: {:?}",
        labels
    );
}

#[tokio::test]
async fn test_completion_input_value_definition_default() {
    let schema = r#"
      enum Status { ACTIVE, INACTIVE }
      type Query {
        user(status: Status): User
      }
      type User { id: ID! }
    "#;
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("type Mutation { updateUser(status: Status = |): User }");
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
async fn test_completion_argument_value_boolean() {
    let schema = r#"
      type Query {
        user(isAdmin: Boolean): User
      }
      type User { id: ID! }
    "#;
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { user(isAdmin: |) { id } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"true"));
    assert!(labels.contains(&"false"));
    assert!(labels.contains(&"null"));
}
