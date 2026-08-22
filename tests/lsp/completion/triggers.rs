use crate::support::{
    completion_items_array, create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    make_temp_project_with_schema, with_cursor, write_project_file,
};
use tower_lsp_server::ls_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_trigger_on_new_empty_line_in_selection() {
    let schema =
        "type Query { users: [User!]! } type User { id: ID! username: String! email: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config = config.with_base_dir(dir.path().to_path_buf());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "query {\n  users {\n    id\n    |\n  }\n}\n";
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "id"));
    assert!(items.iter().any(|i| i.label == "username"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_trigger_after_typing_first_character() {
    let schema =
        "type Query { users: [User!]! } type User { id: ID! username: String! email: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config = config.with_base_dir(dir.path().to_path_buf());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "query {\n  users {\n    id\n    u|\n  }\n}\n";
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let username_item = items
        .iter()
        .find(|i| i.label == "username")
        .expect("Expected 'username' completion");
    assert_eq!(username_item.kind, Some(CompletionItemKind::FIELD));
    assert_eq!(username_item.detail.as_deref(), Some("String!"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_in_completely_empty_selection_set() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config = config.with_base_dir(dir.path().to_path_buf());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "query { users { | } }";
    let (text, position) = with_cursor(text);
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
async fn test_completion_tsx_trigger_on_new_empty_line_in_selection() {
    let schema =
        "type Query { users: [User!]! } type User { id: ID! username: String! email: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config = config.with_base_dir(dir.path().to_path_buf());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text =
        "const q = graphql(/* GraphQL */ `\nquery {\n  users {\n    id\n    |\n  }\n}\n`);\n";
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "test.tsx", &text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "id"));
    assert!(items.iter().any(|i| i.label == "username"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_tsx_trigger_after_typing_first_character() {
    let schema =
        "type Query { users: [User!]! } type User { id: ID! username: String! email: String }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config = config.with_base_dir(dir.path().to_path_buf());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text =
        "const q = graphql(/* GraphQL */ `\nquery {\n  users {\n    id\n    u|\n  }\n}\n`);\n";
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "test.tsx", &text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let username_item = items
        .iter()
        .find(|i| i.label == "username")
        .expect("Expected 'username' completion");
    assert_eq!(username_item.kind, Some(CompletionItemKind::FIELD));
    assert_eq!(username_item.detail.as_deref(), Some("String!"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_tsx_in_completely_empty_selection_set() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config = config.with_base_dir(dir.path().to_path_buf());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "const q = graphql(/* GraphQL */ `\nquery { users { | } }\n`);\n";
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "test.tsx", &text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let username_item = items
        .iter()
        .find(|i| i.label == "username")
        .expect("Expected 'username' completion");
    assert_eq!(username_item.kind, Some(CompletionItemKind::FIELD));
    assert_eq!(username_item.detail.as_deref(), Some("String!"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_operation_type_keywords() {
    let schema = "type Query { id: ID }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "qu|";
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "query"));
    assert!(items.iter().any(|i| i.label == "mutation"));
    assert!(items.iter().any(|i| i.label == "subscription"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_schema_keywords() {
    let schema = "type Query { id: ID }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "ty|";
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "type"));
    assert!(items.iter().any(|i| i.label == "input"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_union_members() {
    let schema = "type A { id: ID } type B { id: ID } type Query { id: ID }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "union MyUnion = A | |";
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "B"));
    assert!(items.iter().any(|i| i.label == "Query"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_implements_interfaces() {
    let schema = "interface Node { id: ID } type Query { id: ID }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "type User implements |";
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "Node"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_directive_arguments() {
    let schema = "directive @myDir(arg1: String, arg2: Int) on FIELD type Query { id: ID }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "query { id @myDir( | ) }";
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "arg1"));
    assert!(items.iter().any(|i| i.label == "arg2"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_field_alias() {
    let schema = "type User { id: ID name: String } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "query { user { alias: | } }";
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(items.iter().any(|i| i.label == "id"));
    assert!(items.iter().any(|i| i.label == "name"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_disabled_inside_inline_comment() {
    let schema = "type Query { users: [User!]! } type User { id: ID! name: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "query {\n  users { # graphox-ignore|\n    id\n  }\n}";
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);
    assert!(items.is_empty(), "Expected no completions inside comments");
}

// ============================================================================
// Additional Keyword Prefix Tests
// ============================================================================

async fn run_keyword_prefix_case(prefix: &str, expected_label: &str) {
    let schema = "type Query { id: ID }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text_with_cursor = format!("{prefix}|");
    let (text, position) = with_cursor(&text_with_cursor);
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(
        items.iter().any(|i| i.label == expected_label),
        "Expected '{expected_label}' for prefix '{prefix}': {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_keyword_prefix_table_driven() {
    let cases: Vec<(&str, &str)> = vec![
        ("mu", "mutation"),
        ("su", "subscription"),
        ("que", "query"),
        ("mut", "mutation"),
        ("sub", "subscription"),
        ("in", "input"),
        ("un", "union"),
        ("en", "enum"),
        ("sc", "scalar"),
        ("ex", "extend"),
        ("di", "directive"),
    ];

    for (prefix, expected_label) in cases {
        run_keyword_prefix_case(prefix, expected_label).await;
    }
}
