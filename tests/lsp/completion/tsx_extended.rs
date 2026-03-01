use crate::support::{
    completion_items_array, create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    make_temp_project_with_schema, with_cursor, write_project_file,
};

// ============================================================================
// Directive Completions in TSX
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_directive_completion_tsx_on_field() {
    let schema =
        "type Query { user: User } type User { id: ID! name: String! } directive @testDir on FIELD";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config = config.with_base_dir(dir.path().to_path_buf());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) =
        with_cursor("const q = graphql(/* GraphQL */ `\nquery { user { id @| }\n}`);");
    let uri = write_project_file(&dir, "test.tsx", &text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(
        items.iter().any(|i| i.label == "testDir"),
        "Expected 'testDir' directive in completions: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_directive_completion_tsx_on_fragment() {
    let schema = "type Query { user: User } type User { id: ID! name: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config = config.with_base_dir(dir.path().to_path_buf());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor(
        "const q = graphql(/* GraphQL */ `\nfragment UserFrag on User @| { id }\nquery { user { ...UserFrag }\n}`);",
    );
    let uri = write_project_file(&dir, "test.tsx", &text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(
        items.iter().any(|i| i.label == "public"),
        "Expected 'public' directive in completions: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert!(
        items.iter().any(|i| i.label == "type_only"),
        "Expected 'type_only' directive in completions: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_directive_completion_tsx_on_operation() {
    let schema = "type Query { user: User } type User { id: ID! } directive @skip on QUERY";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config = config.with_base_dir(dir.path().to_path_buf());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) =
        with_cursor("const q = graphql(/* GraphQL */ `\nquery @| { user { id } }\n`);");
    let uri = write_project_file(&dir, "test.tsx", &text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(
        items.iter().any(|i| i.label == "skip"),
        "Expected 'skip' directive in completions: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ============================================================================
// Union Member Completions in TSX
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_union_member_completion_tsx() {
    let schema = "type Query { item: Item } type A { id: ID! name: String! } type B { id: ID! title: String! } union Item = A | B";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config = config.with_base_dir(dir.path().to_path_buf());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) =
        with_cursor("const q = graphql(/* GraphQL */ `\nquery { item { ... on | }\n}\n`);");
    let uri = write_project_file(&dir, "test.tsx", &text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

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
    assert!(
        labels.contains(&"Item"),
        "Expected 'Item' (union type itself) in completions: {:?}",
        labels
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_union_member_completion_tsx_with_prefix() {
    let schema = "type Query { item: Item } type A { id: ID! name: String! } type AB { id: ID! extra: String! } union Item = A | AB";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config = config.with_base_dir(dir.path().to_path_buf());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) =
        with_cursor("const q = graphql(/* GraphQL */ `\nquery { item { ... on AB| }\n}\n`);");
    let uri = write_project_file(&dir, "test.tsx", &text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"AB"),
        "Expected 'AB' in completions: {:?}",
        labels
    );
    assert!(
        !labels.contains(&"A"),
        "Expected 'A' to be filtered out when prefix is 'AB': {:?}",
        labels
    );
}

// Test the same prefix filtering in standalone .graphql files
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_union_member_completion_graphql_with_prefix() {
    let schema = "type Query { item: Item } type A { id: ID! name: String! } type AB { id: ID! extra: String! } union Item = A | AB";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config = config.with_base_dir(dir.path().to_path_buf());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { item { ... on AB| } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"AB"),
        "Expected 'AB' in completions: {:?}",
        labels
    );
    assert!(
        !labels.contains(&"A"),
        "Expected 'A' to be filtered out when prefix is 'AB': {:?}",
        labels
    );
}

// ============================================================================
// Implements Completions in TSX
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_implements_completion_tsx() {
    let schema =
        "interface Node { id: ID! } interface Named { name: String! } type Query { node: Node }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config = config.with_base_dir(dir.path().to_path_buf());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) =
        with_cursor("const q = graphql(/* GraphQL */ `\ntype User implements | { id: ID! }\n`);");
    let uri = write_project_file(&dir, "test.tsx", &text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"Node"),
        "Expected 'Node' in completions: {:?}",
        labels
    );
    assert!(
        labels.contains(&"Named"),
        "Expected 'Named' in completions: {:?}",
        labels
    );
}

// ============================================================================
// Deeply Nested Fields in TSX
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_deeply_nested_field_completion_tsx() {
    let schema = "type Query { user: User } type User { posts: [Post!]! } type Post { comments: [Comment!]! } type Comment { id: ID! text: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config = config.with_base_dir(dir.path().to_path_buf());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor(
        "const q = graphql(/* GraphQL */ `\nquery {\n  user {\n    posts {\n      comments {\n        |\n      }\n    }\n  }\n}\n`);",
    );
    let uri = write_project_file(&dir, "test.tsx", &text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(
        items.iter().any(|i| i.label == "id"),
        "Expected 'id' field in completions at depth 3: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert!(
        items.iter().any(|i| i.label == "text"),
        "Expected 'text' field in completions at depth 3: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

// ============================================================================
// Variable Completions in TSX
// ============================================================================

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_variable_completion_tsx() {
    let schema = "type Query { user(id: ID!, name: String): User } type User { id: ID! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config = config.with_base_dir(dir.path().to_path_buf());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor(
        "const q = graphql(/* GraphQL */ `\nquery GetUser($id: ID!) { user(id: $|) { id } }\n`);",
    );
    let uri = write_project_file(&dir, "test.tsx", &text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    assert!(
        items.iter().any(|i| i.label == "$id"),
        "Expected '$id' variable in completions: {:?}",
        items.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_variable_default_completion_tsx() {
    let schema = "enum Status { ACTIVE, INACTIVE } type Query { user(status: Status): User } type User { id: ID! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.tsx");
    config = config.with_base_dir(dir.path().to_path_buf());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor(
        "const q = graphql(/* GraphQL */ `\nquery($status: Status = |) { user(status: $status) { id } }\n`);",
    );
    let uri = write_project_file(&dir, "test.tsx", &text);
    lsp_did_open(&mut service, uri.clone(), "typescript", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
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
