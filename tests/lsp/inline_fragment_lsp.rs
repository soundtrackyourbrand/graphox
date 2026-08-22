use crate::support::{
    completion_items_array, create_initialized_lsp_service, lsp_did_open, lsp_request_completion,
    lsp_request_hover, lsp_request_typed, make_temp_project_with_schema, with_cursor,
    write_project_file,
};
use std::fs;
use tower_lsp_server::ls_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_inside_inline_fragment() {
    let (dir, mut config) = make_temp_project_with_schema(
        "\n        type Query { search: [SearchResult!]! }\n        union SearchResult = User | Post\n        type User { id: ID!, username: String! }\n        type Post { id: ID!, title: String! }\n        ",
        "**/*.graphql",
    );
    config = config.with_base_dir(std::fs::canonicalize(dir.path()).unwrap());
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let text = r#"
        query {
            search {
                ... on User {
                    user|name
                }
            }
        }
    "#;
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "query.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_hover(&mut service, uri.clone(), position).await;

    assert!(
        result.is_some(),
        "Hover should return something for 'username' in inline fragment"
    );
    let hover = result.unwrap();
    let HoverContents::Markup(m) = hover.contents else {
        panic!("Expected Markup contents, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("field `User.username`"),
        "Should show field info for User.username, got: {}",
        m.value
    );

    // Hover over 'User' type condition
    let (text2, position2) = with_cursor(
        r#"
        query {
            search {
                ... on Us|er {
                    username
                }
            }
        }
    "#,
    );
    let uri2 = write_project_file(&dir, "query2.graphql", &text2);
    lsp_did_open(&mut service, uri2.clone(), "graphql", 1, &text2).await;

    let result = lsp_request_hover(&mut service, uri2.clone(), position2).await;

    assert!(
        result.is_some(),
        "Hover should return something for 'User' type condition"
    );
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(
            m.value.contains("### type User"),
            "Should show type info for User, got: {}",
            m.value
        );
    } else {
        panic!("Expected Markup contents");
    }
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_inside_inline_fragment() {
    let (dir, mut config) = make_temp_project_with_schema(
        "\n        type Query { search: [SearchResult!]! }\n        union SearchResult = User | Post\n        type User { id: ID!, username: String! }\n        type Post { id: ID!, title: String! }\n        ",
        "**/*.graphql",
    );
    config = config.with_base_dir(std::fs::canonicalize(dir.path()).unwrap());
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let text = r#"
        fragment UserFields on User {
            id
        }
        query {
            search {
                ... on User {
                    ...User|Fields
                }
            }
        }
    "#;
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "query.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    // Go to definition for 'UserFields' inside the inline fragment
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    assert!(
        result.is_some(),
        "Goto definition should return something for 'UserFields' in inline fragment"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_completion_inside_inline_fragment() {
    let (dir, mut config) = make_temp_project_with_schema(
        "\n        type Query { search: [SearchResult!]! }\n        union SearchResult = User | Post\n        type User { id: ID!, username: String! }\n        type Post { id: ID!, title: String! }\n        ",
        "**/*.graphql",
    );
    config = config.with_base_dir(std::fs::canonicalize(dir.path()).unwrap());
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let text = r#"
        query {
            search {
                ... on User {
                    |
                }
            }
        }
    "#;
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "query.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    // Completion inside the inline fragment
    let result = lsp_request_completion(&mut service, uri.clone(), position).await;
    let items = completion_items_array(&result);

    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(
        labels.contains(&"username"),
        "Completions should include 'username', got: {:?}",
        labels
    );
    assert!(
        labels.contains(&"id"),
        "Completions should include 'id', got: {:?}",
        labels
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_inside_inline_fragment() {
    let (dir, mut config) = make_temp_project_with_schema(
        "\n        type Query { search: [SearchResult!]! }\n        union SearchResult = User | Post\n        type User { id: ID!, username: String! }\n        type Post { id: ID!, title: String! }\n        ",
        "**/*.graphql",
    );
    config = config.with_base_dir(std::fs::canonicalize(dir.path()).unwrap());
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let text = r#"
        fragment User|Fields on User {
            id
        }
        query {
            search {
                ... on User {
                    ...UserFields
                }
            }
        }
    "#;
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "query.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    // Find references for 'UserFields'
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    let locations = result.expect("Expected locations");
    // Should find the definition and the spread inside the inline fragment
    assert!(
        locations.len() >= 2,
        "Should find at least 2 locations, got: {}",
        locations.len()
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_rename_inside_inline_fragment() {
    let (dir, mut config) = make_temp_project_with_schema(
        "\n        type Query { search: [SearchResult!]! }\n        union SearchResult = User | Post\n        type User { id: ID!, username: String! }\n        type Post { id: ID!, title: String! }\n        ",
        "**/*.graphql",
    );
    config = config.with_base_dir(std::fs::canonicalize(dir.path()).unwrap());
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let text = r#"
        fragment User|Fields on User {
            id
        }
        query {
            search {
                ... on User {
                    ...UserFields
                }
            }
        }
    "#;
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "query.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    // Rename 'UserFields'
    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        new_name: "RenamedUserFields".to_string(),
        work_done_progress_params: Default::default(),
    };

    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;

    let edit = result.expect("Expected workspace edit");
    let changes = edit.changes.expect("Expected changes");
    let file_changes = changes.get(&uri).expect("Expected changes for file");

    assert_eq!(
        file_changes.len(),
        2,
        "Should have 2 changes (definition and spread), got: {:?}",
        file_changes
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_definition_field_in_schema() {
    let schema_text = "\n        type Query { search: [SearchResult!]! }\n        union SearchResult = User | Post\n        type User { id: ID!, username: String! }\n        type Post { id: ID!, title: String! }\n        ";
    let (dir, mut config) = make_temp_project_with_schema(schema_text, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(dir.path()).unwrap());
    let (mut service, _) = create_initialized_lsp_service(config).await;

    // Open schema first so it's in documents
    let schema_path = dir.path().join("schema.graphql");
    let schema_uri = Uri::from_file_path(fs::canonicalize(&schema_path).unwrap()).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, schema_text).await;

    let text = r#"
        query {
            search {
                ... on User {
                    user|name
                }
            }
        }
    "#;
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "query.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    // Go to definition for 'username'
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    assert!(
        result.is_some(),
        "Goto definition should return something for 'username'"
    );
    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        let expected_path = schema_uri.path().as_str().to_lowercase();
        let actual_path = loc.uri.path().as_str().to_lowercase();
        // Handle macOS /private/var vs /var
        let expected_path = expected_path.trim_start_matches("/private");
        let actual_path = actual_path.trim_start_matches("/private");
        assert_eq!(expected_path, actual_path);
    } else {
        panic!("Expected Scalar(Location)");
    }
}
