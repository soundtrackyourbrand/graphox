use crate::support::{
    completion_items_array, create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    make_temp_project_with_schema, with_cursor, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_completion_selection_set_type_filtering() {
    let schema = "type Query { users: [User!]! posts: [Post!]! } type User { id: ID! username: String! } type Post { id: ID! title: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, positions) = crate::support::with_cursors("query { users { | } posts { | } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), positions[0]).await;
    let items = completion_items_array(&result);
    assert!(items.iter().any(|i| i.label == "username"));
    assert!(!items.iter().any(|i| i.label == "title"));

    let result = lsp_request_completion(&mut service, uri.clone(), positions[1]).await;
    let items = completion_items_array(&result);
    assert!(items.iter().any(|i| i.label == "title"));
    assert!(!items.iter().any(|i| i.label == "username"));
}

#[tokio::test]
async fn test_fragment_spread_interface_filtering() {
    let schema = "type Query { nodeA: A nodeB: B } interface Node { id: ID! } type A implements Node { id: ID! name: String! } type B implements Node { id: ID! title: String! }";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "fragment OnNode on Node { id } fragment OnA on A { name } fragment OnB on B { title } query { nodeA { ...| } nodeB { ...| } }";
    let (text, positions) = crate::support::with_cursors(text);
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), positions[0]).await;
    let items = completion_items_array(&result);
    assert!(items.iter().any(|i| i.label == "OnA"));
    assert!(items.iter().any(|i| i.label == "OnNode"));
    assert!(!items.iter().any(|i| i.label == "OnB"));
    assert!(
        !items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::FIELD))
    );

    let result = lsp_request_completion(&mut service, uri.clone(), positions[1]).await;
    let items = completion_items_array(&result);
    assert!(items.iter().any(|i| i.label == "OnB"));
    assert!(items.iter().any(|i| i.label == "OnNode"));
    assert!(!items.iter().any(|i| i.label == "OnA"));
    assert!(
        !items
            .iter()
            .any(|i| i.kind == Some(CompletionItemKind::FIELD))
    );
}

#[tokio::test]
async fn test_fragment_spread_union_filtering_extended() {
    let schema = "type Query { itemA: A itemB: B } type A { id: ID! name: String! } type B { id: ID! title: String! } union Item = A | B";
    let (dir, mut config) = make_temp_project_with_schema(schema, "test.graphql");
    config.base_dir = dir.path().to_path_buf();

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = "fragment OnItem on Item { id } fragment OnA on A { name } fragment OnB on B { title } query { itemA { ...| } itemB { ...| } }";
    let (text, positions) = crate::support::with_cursors(text);
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), positions[0]).await;
    if let Some(CompletionResponse::Array(items)) = result {
        assert!(items.iter().any(|i| i.label == "OnA"));
        assert!(items.iter().any(|i| i.label == "OnItem"));
        assert!(!items.iter().any(|i| i.label == "OnB"));
    } else {
        panic!("Expected array of completions");
    }

    let result = lsp_request_completion(&mut service, uri.clone(), positions[1]).await;
    if let Some(CompletionResponse::Array(items)) = result {
        assert!(items.iter().any(|i| i.label == "OnB"));
        assert!(items.iter().any(|i| i.label == "OnItem"));
        assert!(!items.iter().any(|i| i.label == "OnA"));
    } else {
        panic!("Expected array of completions");
    }
}

#[tokio::test]
async fn test_completion_inside_union_type() {
    let schema = "type Query { node: Item } type A { id: ID! name: String! } type B { id: ID! title: String! } union Item = A | B";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { node { | } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
    assert!(
        items.iter().any(|i| i.label == "__typename"),
        "Should include __typename for union type: {:?}",
        labels
    );
    assert!(
        !items.iter().any(|i| i.label == "id"),
        "Should NOT include 'id' directly - union requires inline fragment: {:?}",
        labels
    );
    assert!(
        !items.iter().any(|i| i.label == "name"),
        "Should NOT include 'name' directly - union requires inline fragment: {:?}",
        labels
    );
    assert!(
        !items.iter().any(|i| i.label == "title"),
        "Should NOT include 'title' directly - union requires inline fragment: {:?}",
        labels
    );
}

#[tokio::test]
async fn test_completion_union_with_inline_fragments() {
    let schema = "type Query { node: Item } type A { id: ID! name: String! } type B { id: ID! title: String! } union Item = A | B";
    let (dir, config) = make_temp_project_with_schema(schema, "test.graphql");

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let (text, position) = with_cursor("query { node { ... on | } }");
    let uri = write_project_file(&dir, "test.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
    assert!(
        items.iter().any(|i| i.label == "A"),
        "Should offer union member A: {:?}",
        labels
    );
    assert!(
        items.iter().any(|i| i.label == "B"),
        "Should offer union member B: {:?}",
        labels
    );
    assert!(
        items.iter().any(|i| i.label == "Item"),
        "Should offer union type itself: {:?}",
        labels
    );
}
