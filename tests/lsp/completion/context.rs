use crate::support::{
    LspBackend, completion_items_array, create_initialized_lsp_service, lsp_did_open,
    lsp_request_completion, make_temp_project_with_schema, with_cursor, write_project_file,
};
use tempfile::TempDir;
use tower_lsp::lsp_types::*;

async fn make_completion_fixture(
    schema: &str,
    fixture: &str,
) -> (
    tower_lsp::LspService<LspBackend>,
    Url,
    Position,
    TempDir,
    tokio::task::JoinHandle<()>,
) {
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, handle) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor(fixture);
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;
    (service, uri, position, dir, handle)
}

// ============================================================================
// Fragment Spread Context - After three dots
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_context_after_three_dots() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (mut service, uri, position, _dir, _handle) = make_completion_fixture(
        schema,
        "fragment UserFrag on User { id } query { user { ...| } }",
    )
    .await;

    let result = lsp_request_completion(&mut service, uri, position).await;
    let items = completion_items_array(&result);

    // Should show fragment spread completions
    assert!(
        items.iter().any(|i| i.label == "UserFrag"),
        "Expected fragment in completions: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_context_after_three_dots_with_space() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (mut service, uri, position, _dir, _handle) =
        make_completion_fixture(schema, "query { user { ... | } }").await;

    let result = lsp_request_completion(&mut service, uri, position).await;
    let items = completion_items_array(&result);

    // Should show type completions
    assert!(!items.is_empty(), "Expected completions after '... '");
}

// ============================================================================
// Inline Fragment Context - After "on" keyword
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_context_after_on_keyword() {
    let schema =
        "type Query { item: Item } type A { id: ID! } type B { name: String! } union Item = A | B";
    let (mut service, uri, position, _dir, _handle) =
        make_completion_fixture(schema, "query { item { ... on | } }").await;

    let result = lsp_request_completion(&mut service, uri, position).await;
    let items = completion_items_array(&result);

    // Should show union members and the union type itself
    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"A"),
        "Expected 'A' in completions: {:?}",
        labels
    );
    assert!(
        labels.contains(&"B"),
        "Expected 'B' in completions: {:?}",
        labels
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_context_after_on_with_prefix() {
    let schema = "type Query { item: Item } type ABC { id: ID! } type XYZ { name: String! } union Item = ABC | XYZ";
    let (mut service, uri, position, _dir, _handle) =
        make_completion_fixture(schema, "query { item { ... on X| } }").await;

    let result = lsp_request_completion(&mut service, uri, position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    // Should only show XYZ (starts with X), not ABC
    assert!(
        labels.contains(&"XYZ"),
        "Expected 'XYZ' in completions: {:?}",
        labels
    );
    assert!(
        !labels.contains(&"ABC"),
        "Expected 'ABC' to be filtered: {:?}",
        labels
    );
}

// ============================================================================
// Union Type Context - After pipe
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_context_after_pipe() {
    let schema =
        "type Query { item: Item } type A { id: ID! } type B { name: String! } union Item = A | B";
    let (mut service, uri, position, _dir, _handle) =
        make_completion_fixture(schema, "union Item = A | |").await;

    let result = lsp_request_completion(&mut service, uri, position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"B"),
        "Expected 'B' in completions: {:?}",
        labels
    );
}

// ============================================================================
// Variable Default Context - After equals
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_context_after_equals() {
    let schema = "enum Status { ACTIVE, INACTIVE } type Query { user(status: Status): User } type User { id: ID! }";
    let (mut service, uri, position, _dir, _handle) = make_completion_fixture(
        schema,
        "query($status: Status = |) { user(status: $status) { id } }",
    )
    .await;

    let result = lsp_request_completion(&mut service, uri, position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"ACTIVE"),
        "Expected 'ACTIVE' in completions: {:?}",
        labels
    );
    assert!(
        labels.contains(&"INACTIVE"),
        "Expected 'INACTIVE' in completions: {:?}",
        labels
    );
    assert!(
        labels.contains(&"null"),
        "Expected 'null' in completions: {:?}",
        labels
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_context_after_equals_in_variable_def() {
    let schema = "enum Status { ACTIVE, INACTIVE } type Query { user: User } type User { id: ID! }";
    let (mut service, uri, position, _dir, _handle) =
        make_completion_fixture(schema, "query($status: Status = |) { user { id } }").await;

    let result = lsp_request_completion(&mut service, uri, position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"ACTIVE"));
    assert!(labels.contains(&"INACTIVE"));
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_context_after_equals_in_argument() {
    let schema = "enum Status { ACTIVE, INACTIVE } type Query { user(status: Status): User } type User { id: ID! }";
    let (mut service, uri, position, _dir, _handle) =
        make_completion_fixture(schema, "query { user(status: |) { id } }").await;

    let result = lsp_request_completion(&mut service, uri, position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"ACTIVE"));
    assert!(labels.contains(&"INACTIVE"));
}

// ============================================================================
// Alias Context - After colon in selection
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_context_after_colon_in_selection() {
    let schema = "type Query { user: User } type User { id: ID! name: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Cursor after ":" in alias position should offer field completions
    let (text, position) = with_cursor("query { user { myAlias: | } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    // Should show available fields
    assert!(items.iter().any(|i| i.label == "id"));
    assert!(items.iter().any(|i| i.label == "name"));
}
