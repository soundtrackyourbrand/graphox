use tower_lsp_server::ls_types::*;

use crate::support::{
    completion_items_array, create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    make_temp_project_with_schema, with_cursor, write_project_file,
};

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_fields() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { users { | } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let id_item = items
        .iter()
        .find(|i| i.label == "id")
        .expect("Expected 'id' completion");
    assert_eq!(id_item.kind, Some(CompletionItemKind::FIELD));
    assert_eq!(id_item.detail.as_deref(), Some("ID!"));

    let username_item = items
        .iter()
        .find(|i| i.label == "username")
        .expect("Expected 'username' completion");
    assert_eq!(username_item.kind, Some(CompletionItemKind::FIELD));
    assert_eq!(username_item.detail.as_deref(), Some("String!"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_inline_fragment_completion_inserts_braces_when_missing() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query {\n  users {\n    ... on |\n  }\n}\n");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let item = items
        .iter()
        .find(|i| i.label == "User")
        .expect("Expected 'User' completion");

    let (final_text, _pos) = crate::support::apply_completion_item(&text, position, item);

    assert_eq!(
        final_text,
        "query {\n  users {\n    ... on User {\n      \n    }\n  }\n}\n"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_inline_fragment_completion_no_braces_when_present() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query {\n  users {\n    ... on | { id }\n  }\n}\n");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let item = items
        .iter()
        .find(|i| i.label == "User")
        .expect("Expected 'User' completion");

    let (final_text, _pos) = crate::support::apply_completion_item(&text, position, item);

    assert_eq!(
        final_text,
        "query {\n  users {\n    ... on User { id }\n  }\n}\n"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_partial_input() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { users { usern| } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "username"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_field_completion_inserts_braces_when_missing() {
    let schema = "type Query { user: User } type User { id: ID! username: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { user| }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let item = items
        .iter()
        .find(|i| i.label == "user")
        .expect("Expected 'user' completion");

    let (final_text, new_pos) = crate::support::apply_completion_item(&text, position, item);

    assert!(
        final_text.contains("user {"),
        "Expected braces in completion: {:?}",
        final_text
    );

    if let Some(pos) = new_pos {
        assert_eq!(pos, Position::new(1, 2));
    } else if let Some(insert_text) = &item.insert_text
        && insert_text.contains("$0")
    {
        panic!("Expected new_pos to be Some when snippet is applied");
    }
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_field_completion_no_braces_when_present() {
    let schema = "type Query { user: User } type User { id: ID! username: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { user| { id } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let item = items
        .iter()
        .find(|i| i.label == "user")
        .expect("Expected 'user' completion");

    if item.text_edit.is_none() {
        let insert_text = item.insert_text.as_ref().unwrap_or(&item.label);
        assert!(
            !insert_text.contains('{'),
            "Should not add braces when already present"
        );
    }
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_field_completion_no_braces_for_scalar() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { user { | } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let item = items
        .iter()
        .find(|i| i.label == "id")
        .expect("Expected 'id' completion");

    if item.text_edit.is_none() {
        let insert_text = item.insert_text.as_ref().unwrap_or(&item.label);
        assert!(
            !insert_text.contains('{'),
            "Scalar field should not have braces"
        );
    }
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_field_completion_nested_indentation() {
    let schema =
        "type Query { user: User } type User { posts: [Post!]! } type Post { title: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query {\n  user {\n    posts|\n  }\n}");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let item = items
        .iter()
        .find(|i| i.label == "posts")
        .expect("Expected 'posts' completion");

    let (final_text, _pos) = crate::support::apply_completion_item(&text, position, item);

    assert!(
        final_text.contains("posts {\n      \n    }"),
        "Expected proper indentation: {:?}",
        final_text
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_field_completion_interface_return_type() {
    let schema = "type Query { node: Node } interface Node { id: ID! } type User implements Node { id: ID! name: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { node| }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let item = items
        .iter()
        .find(|i| i.label == "node")
        .expect("Expected 'node' completion");

    let (final_text, _pos) = crate::support::apply_completion_item(&text, position, item);

    assert!(
        final_text.contains("node {"),
        "Interface-returning field should have braces: {:?}",
        final_text
    );
}
