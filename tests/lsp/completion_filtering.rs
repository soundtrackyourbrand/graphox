use crate::support::{
    completion_items_array, create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    make_temp_project_with_schema, pos, write_project_file,
};

#[tokio::test]
async fn test_completion_fragment_spread_filtering() {
    let schema = "type Query { user: User posts: [Post] } type User { id: ID! } type Post { id: ID! title: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = r#"
        fragment UserFields on User { id }
        fragment PostFields on Post { id title }
        
        query {
            user {
                ...
            }
        }
    "#;
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // Request completions at "user { ...| }"
    let result = lsp_request_completion(&mut service, uri.clone(), pos(6, 19)).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();

    assert!(
        items.iter().any(|i| i.label == "UserFields"),
        "Should suggest UserFields on User type. Found: {:?}",
        labels
    );
    assert!(
        !items.iter().any(|i| i.label == "PostFields"),
        "Should NOT suggest PostFields on User type. Found: {:?}",
        labels
    );
}

#[tokio::test]
async fn test_completion_fragment_spread_interface_filtering() {
    let schema = "interface Node { id: ID! } type User implements Node { id: ID! name: String } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = r#"
        fragment NodeFields on Node { id }
        fragment UserFields on User { name }
        
        query {
            user {
                ...
            }
        }
    "#;
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), pos(6, 19)).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();

    assert!(
        items.iter().any(|i| i.label == "NodeFields"),
        "Should suggest NodeFields on User type (interface). Found: {:?}",
        labels
    );
    assert!(
        items.iter().any(|i| i.label == "UserFields"),
        "Should suggest UserFields on User type. Found: {:?}",
        labels
    );
}

#[tokio::test]
async fn test_completion_fragment_spread_union_filtering() {
    let schema = "type User { id: ID! } type Guest { name: String } union Actor = User | Guest type Query { actor: Actor }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = r#"
        fragment UserFields on User { id }
        fragment ActorFields on Actor { __typename }
        
        query {
            actor {
                ...
            }
        }
    "#;
    let uri = write_project_file(&dir, "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), pos(6, 19)).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();

    assert!(
        items.iter().any(|i| i.label == "UserFields"),
        "Should suggest UserFields on Actor union (member). Found: {:?}",
        labels
    );
    assert!(
        items.iter().any(|i| i.label == "ActorFields"),
        "Should suggest ActorFields on Actor union. Found: {:?}",
        labels
    );

    // Test reverse: spread union fragment into object member
    let updated_text = r#"
        fragment UserFields on User { id }
        fragment GuestFields on Guest { name }
        fragment ActorFields on Actor { __typename }
        
        query {
            user: actor {
                ... on User {
                    ...
                }
            }
        }
    "#;
    // We don't have a lsp_did_change helper yet, but we can use lsp_did_open with version 2 or just didOpen again.
    // Actually, lsp_did_open works fine for overwriting in these tests.
    lsp_did_open(&mut service, uri.clone(), "graphql", 2, updated_text).await;

    let result = lsp_request_completion(&mut service, uri.clone(), pos(8, 23)).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| &i.label).collect();
    assert!(
        items.iter().any(|i| i.label == "ActorFields"),
        "Should suggest ActorFields inside User (member of union). Found: {:?}",
        labels
    );
    assert!(
        items.iter().any(|i| i.label == "UserFields"),
        "Should suggest UserFields inside User. Found: {:?}",
        labels
    );
    assert!(
        !items.iter().any(|i| i.label == "GuestFields"),
        "Should NOT suggest GuestFields inside User. Found: {:?}",
        labels
    );
}
