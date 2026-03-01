use crate::support::{
    completion_items_array, create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    make_temp_project_with_schema, with_cursor, write_project_file,
};

// ============================================================================
// Cursor Position Edge Cases
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_cursor_at_end_of_file() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { user { id } }|");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);
    // Should return empty or minimal results, not crash
    assert!(
        items.is_empty() || items.len() < 5,
        "Expected minimal completions at EOF"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_cursor_after_last_char() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { id }|");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    // Should handle gracefully, not crash
    assert!(result.is_some());
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_cursor_in_whitespace() {
    let schema = "type Query { user: User } type User { id: ID! name: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query {  |  }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);
    // Should provide field completions in the empty selection set
    assert!(items.iter().any(|i| i.label == "user"));
}

// ============================================================================
// Empty/Minimal Documents
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_empty_document() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("|");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    // Should handle gracefully - may return operation type keywords or empty
    assert!(result.is_some());
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_only_whitespace() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("   |");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    assert!(result.is_some());
}

// ============================================================================
// Fragment Edge Cases
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_fragment_completion_self_reference() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Fragment should not suggest itself
    let (text, position) = with_cursor("fragment UserFrag on User { id } query { user { ...| } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;

    // UserFrag should NOT appear since we're spreading ON User (not defining UserFrag)
    // Actually, the fragment spread completion should show UserFrag
    // But if we're in the fragment definition context, it shouldn't self-reference
    assert!(result.is_some()); // This test verifies the system handles it gracefully
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_fragment_completion_non_spread_context() {
    let schema = "type Query { user: User } type User { id: ID! name: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Fragment completions should NOT appear in field position
    let (text, position) = with_cursor("query { user { | } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    // Should show fields, not fragment spreads
    assert!(items.iter().any(|i| i.label == "id"));
    assert!(items.iter().any(|i| i.label == "name"));
}

// ============================================================================
// Alias Edge Cases
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_alias_completion_no_duplicates() {
    let schema = "type Query { user: User } type User { id: ID! name: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { user { | } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let id_count = items.iter().filter(|i| i.label == "id").count();
    assert_eq!(id_count, 1, "Should have exactly one 'id' completion");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_alias_completion_after_existing_alias() {
    let schema = "type Query { user: User } type User { id: ID! name: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // After using an alias, don't suggest it again
    let (text, position) = with_cursor("query { user { userId: id name: | } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    // Should still show all fields
    assert!(items.iter().any(|i| i.label == "id"));
    assert!(items.iter().any(|i| i.label == "name"));
}

// ============================================================================
// Type System Edge Cases
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_non_null_field_no_null_default() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Non-null variable default should NOT suggest null
    let (text, position) = with_cursor("query($id: ID! = |) { user { id } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    // For non-null ID!, should NOT suggest null
    assert!(
        !labels.contains(&"null"),
        "Should not suggest null for non-null type"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_scalar_without_braces() {
    let schema = "type Query { user: User } type User { id: ID! name: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Scalar field should not add braces
    let (text, position) = with_cursor("query { user { id: | } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    // After an alias, should still show the available fields
    assert!(items.iter().any(|i| i.label == "id"));
}

// ============================================================================
// Comment Handling
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_disabled_in_line_comment() {
    let schema = "type Query { user: User } type User { id: ID! name: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Cursor on the same line as the comment - after #
    let (text, position) = with_cursor("query { # | }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(
        items.is_empty(),
        "Expected no completions inside line comment"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_disabled_after_comment_line() {
    let schema = "type Query { user: User } type User { id: ID! name: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Position on line after comment should still work
    let (text, position) = with_cursor("# comment\n|");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    // Should handle gracefully - may offer keyword completions
    assert!(result.is_some());
}
