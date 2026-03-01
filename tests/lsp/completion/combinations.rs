use crate::support::{
    completion_items_array, create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    make_temp_project_with_schema, with_cursor, write_project_file,
};

// ============================================================================
// Complex Combinations
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_fragment_with_directive_in_tsx() {
    let schema = "type Query { user: User } type User { id: ID! name: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.tsx");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Fragment spread with directive in TSX - should show built-in codegen directives
    let (text, position) = with_cursor(
        "const q = graphql(/* GraphQL */ `\nfragment UserFrag on User { id }\nquery { user { ...UserFrag @| }\n}`);",
    );
    let uri = write_project_file(&dir, "test.tsx", &text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    // Should show directive completions (built-in codegen directives)
    assert!(
        items.iter().any(|i| i.label == "public"),
        "Expected 'public' directive: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_field_with_alias_and_directive() {
    let schema =
        "type Query { user: User } type User { id: ID! name: String! } directive @skip on FIELD";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Field with alias and directive
    let (text, position) = with_cursor("query { user { myId: id @| } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    // Should show directive completions after alias
    assert!(
        items.iter().any(|i| i.label == "skip"),
        "Expected 'skip' directive: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_inline_fragment_with_directive() {
    let schema = "type Query { item: Item } type A { id: ID! } type B { name: String! } union Item = A | B directive @skip on INLINE_FRAGMENT";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Inline fragment with directive
    let (text, position) = with_cursor("query { item { ... on A @| { id } } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    // Should show directive completions
    assert!(
        items.iter().any(|i| i.label == "skip"),
        "Expected 'skip' directive: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ============================================================================
// Partial Input Variations
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_partial_typing_various_positions() {
    let schema = "type Query { user: User } type User { id: ID! username: String! email: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Partial typing "u" - filtering may be case insensitive
    let (text, position) = with_cursor("query { user { u| } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    // At minimum, username should be present (starts with 'u')
    assert!(
        items.iter().any(|i| i.label == "username"),
        "Expected 'username': {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_partial_typing_at_different_depths() {
    let schema = "type Query { user: User } type User { profile: Profile } type Profile { name: String! nickname: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Partial typing "n" at depth 2
    let (text, position) = with_cursor("query { user { profile { n| } } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(
        items.iter().any(|i| i.label == "name"),
        "Expected 'name': {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert!(
        items.iter().any(|i| i.label == "nickname"),
        "Expected 'nickname': {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ============================================================================
// Case Sensitivity
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_prefix_filtering_case_sensitivity() {
    let schema = "type Query { user: User } type User { id: ID! userName: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Lowercase "u" - check that completion filtering works
    let (text, position) = with_cursor("query { user { u| } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    // userName starts with lowercase 'u'
    assert!(
        items.iter().any(|i| i.label == "userName"),
        "Expected 'userName' for lowercase 'u': {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ============================================================================
// Error Recovery
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_after_syntax_error() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Invalid syntax - unbalanced brace, but we still want to trigger completion
    let (text, position) = with_cursor("query { user { id| }"); // missing closing braces
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    // Should handle gracefully, not crash
    assert!(
        result.is_some(),
        "Expected completion result even with syntax error"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_in_invalid_context() {
    let schema = "type Query { user: User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Cursor in clearly invalid position (inside a type definition where fields shouldn't be)
    let (text, position) = with_cursor("type User { | }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    // Should handle gracefully - may return input fields or empty
    assert!(
        result.is_some(),
        "Expected completion result in type definition"
    );
}
