use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    pos, write_project_file_at,
};
use graphox::Config;
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::time::{Duration, sleep};
use tower_lsp_server::ls_types::*;
use tower_service::Service;

/// Helper to create a test config with a schema
fn create_test_config(dir: &std::path::Path) -> Config {
    let schema_text = r#"
        type Query {
            users: [User!]!
            user(id: ID!): User
            posts: [Post!]!
            post(id: ID!): Post
            comments: [Comment!]!
        }

        type User {
            id: ID!
            username: String!
            email: String!
            posts: [Post!]!
            comments: [Comment!]!
        }

        type Post {
            id: ID!
            title: String!
            content: String!
            author: User!
            comments: [Comment!]!
        }

        type Comment {
            id: ID!
            text: String!
            author: User!
            post: Post!
        }

        type Mutation {
            createUser(username: String!, email: String!): User
            createPost(title: String!, content: String!): Post
            createComment(postId: ID!, text: String!): Comment
        }
        "#;

    let (_, mut config) = make_temp_project_with_schema(schema_text, "**/*.graphql");
    config = config.with_base_dir(dir.to_path_buf());
    config = config.with_enable_schema_cache(false);
    config
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_concurrent_document_operations() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (service, _handle) = create_initialized_lsp_service(config).await;

    // Create multiple GraphQL files
    let file_count = 20;
    let mut uris: Vec<(Uri, String)> = Vec::new();

    for i in 0..file_count {
        let query_path = dir.path().join(format!("query_{}.graphql", i));
        let text = format!(
            r#"
            fragment UserFields{} on User {{
                id
                username
                email
            }}

            query GetUser{} {{
                users {{
                    ...UserFields{}
                }}
            }}
            "#,
            i, i, i
        );
        fs::write(&query_path, &text).unwrap();
        let query_path = std::fs::canonicalize(query_path).unwrap();
        let uri = Uri::from_file_path(&query_path).unwrap();
        uris.push((uri, text));
    }

    // Concurrently open all documents
    let service_arc = Arc::new(tokio::sync::Mutex::new(service));
    let mut tasks = Vec::new();

    for (uri, text) in uris.clone() {
        let service = Arc::clone(&service_arc);
        let task = tokio::spawn(async move {
            let mut svc = service.lock().await;
            lsp_did_open(&mut svc, uri, "graphql", 1, &text).await;
            Ok::<(), tower_lsp_server::jsonrpc::Error>(())
        });
        tasks.push(task);
    }

    // Wait for all opens to complete
    for task in tasks {
        task.await.unwrap().unwrap();
    }

    // Allow a brief moment for async processing
    sleep(Duration::from_millis(10)).await;

    println!("All documents opened successfully");

    // Now perform concurrent LSP operations on all documents
    let mut tasks = Vec::new();

    for (uri, _) in uris.iter() {
        let service = Arc::clone(&service_arc);
        let uri = uri.clone();

        // Hover request
        let task = tokio::spawn(async move {
            let params = HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: pos(3, 20), // Over "username"
                },
                work_done_progress_params: Default::default(),
            };

            let mut svc = service.lock().await;
            let result: Option<Hover> =
                lsp_request_typed(&mut svc, "textDocument/hover", &params).await;
            Ok::<Option<Hover>, tower_lsp_server::jsonrpc::Error>(result)
        });
        tasks.push(task);
    }

    // Wait for all hover requests to complete
    for task in tasks {
        let result = task.await.unwrap();
        assert!(result.is_ok(), "Hover request should succeed");
    }

    println!("All hover requests completed successfully");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_concurrent_completion_requests() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Create a test file
    let text = "query { users {  } }";
    let uri = write_project_file_at(dir.path(), "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // Allow a brief moment for async processing
    sleep(Duration::from_millis(10)).await;

    let service_arc = Arc::new(tokio::sync::Mutex::new(service));
    let mut tasks = Vec::new();

    // Fire 50 concurrent completion requests at the same position
    for _ in 0..50 {
        let service = Arc::clone(&service_arc);
        let uri = uri.clone();

        let task = tokio::spawn(async move {
            let params = CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: pos(0, 16), // Inside "users { | }"
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: None,
            };

            let mut svc = service.lock().await;
            let result = lsp_request_typed::<CompletionResponse, _>(
                &mut svc,
                "textDocument/completion",
                &params,
            )
            .await;
            Ok::<CompletionResponse, tower_lsp_server::jsonrpc::Error>(result)
        });
        tasks.push(task);
    }

    // Wait for all completion requests
    let mut success_count = 0;
    for task in tasks {
        let result = task.await.unwrap();
        if result.is_ok() {
            success_count += 1;
        }
    }

    println!(
        "Completed {}/50 concurrent completion requests",
        success_count
    );
    assert_eq!(success_count, 50, "All completion requests should succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_concurrent_mixed_operations() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Create multiple files
    let file_count = 10;
    let mut uris: Vec<(Uri, String)> = Vec::new();

    for i in 0..file_count {
        let text = format!(
            r#"
            fragment PostFields{} on Post {{
                id
                title
                content
            }}

            query GetPosts{} {{
                posts {{
                    ...PostFields{}
                    author {{
                        id
                        username
                    }}
                }}
            }}
            "#,
            i, i, i
        );
        let uri = write_project_file_at(dir.path(), &format!("file_{}.graphql", i), &text);
        uris.push((uri, text));
    }

    // Open all documents
    for (uri, text) in &uris {
        lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;
    }

    // Allow a brief moment for async processing
    sleep(Duration::from_millis(10)).await;

    let service_arc = Arc::new(tokio::sync::Mutex::new(service));
    let mut tasks = Vec::new();

    // Mix different types of concurrent requests
    for (i, (uri, _)) in uris.iter().enumerate() {
        let service = Arc::clone(&service_arc);
        let uri = uri.clone();

        // Hover
        tasks.push(tokio::spawn({
            let service = Arc::clone(&service);
            let uri = uri.clone();
            async move {
                let params = HoverParams {
                    text_document_position_params: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri },
                        position: pos(3, 20),
                    },
                    work_done_progress_params: Default::default(),
                };
                let mut svc = service.lock().await;
                lsp_request_typed::<Option<Hover>, _>(&mut svc, "textDocument/hover", &params)
                    .await;
                Ok::<(), tower_lsp_server::jsonrpc::Error>(())
            }
        }));

        // Completion
        tasks.push(tokio::spawn({
            let service = Arc::clone(&service);
            let uri = uri.clone();
            async move {
                let params = CompletionParams {
                    text_document_position: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri },
                        position: pos(3, 20),
                    },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                    context: None,
                };
                let mut svc = service.lock().await;
                lsp_request_typed::<CompletionResponse, _>(
                    &mut svc,
                    "textDocument/completion",
                    &params,
                )
                .await;
                Ok::<(), tower_lsp_server::jsonrpc::Error>(())
            }
        }));

        // Definition
        tasks.push(tokio::spawn({
            let service = Arc::clone(&service);
            let uri = uri.clone();
            async move {
                let params = GotoDefinitionParams {
                    text_document_position_params: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri },
                        position: pos(10, 25), // Over fragment spread
                    },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                };
                let mut svc = service.lock().await;
                lsp_request_typed::<Option<GotoDefinitionResponse>, _>(
                    &mut svc,
                    "textDocument/definition",
                    &params,
                )
                .await;
                Ok::<(), tower_lsp_server::jsonrpc::Error>(())
            }
        }));

        // References
        tasks.push(tokio::spawn({
            let service = Arc::clone(&service);
            let uri = uri.clone();
            async move {
                let params = ReferenceParams {
                    text_document_position: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri },
                        position: pos(2, 25), // Over fragment name
                    },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                    context: ReferenceContext {
                        include_declaration: true,
                    },
                };
                let mut svc = service.lock().await;
                lsp_request_typed::<Option<Vec<Location>>, _>(
                    &mut svc,
                    "textDocument/references",
                    &params,
                )
                .await;
                Ok::<(), tower_lsp_server::jsonrpc::Error>(())
            }
        }));

        // Document symbols (every other file)
        if i % 2 == 0 {
            tasks.push(tokio::spawn({
                let service = Arc::clone(&service);
                let uri = uri.clone();
                async move {
                    let params = DocumentSymbolParams {
                        text_document: TextDocumentIdentifier { uri },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    };
                    let mut svc = service.lock().await;
                    lsp_request_typed::<Option<DocumentSymbolResponse>, _>(
                        &mut svc,
                        "textDocument/documentSymbol",
                        &params,
                    )
                    .await;
                    Ok::<(), tower_lsp_server::jsonrpc::Error>(())
                }
            }));
        }
    }

    println!("Executing {} concurrent mixed operations", tasks.len());

    // Wait for all requests to complete
    let mut success_count = 0;
    let mut error_count = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(_) => success_count += 1,
            Err(e) => {
                error_count += 1;
                eprintln!("Request failed: {:?}", e);
            }
        }
    }

    println!(
        "Completed {} successful, {} errors out of {} total requests",
        success_count,
        error_count,
        success_count + error_count
    );
    assert!(
        error_count == 0,
        "No requests should fail due to lock contention"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_concurrent_document_changes() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let initial_text = r#"
        fragment UserFields on User {
            id
        }

        query {
            users {
                ...UserFields
            }
        }
    "#;
    let uri = write_project_file_at(dir.path(), "test.graphql", initial_text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, initial_text).await;

    let service_arc = Arc::new(tokio::sync::Mutex::new(service));
    let mut tasks = Vec::new();

    // Simulate rapid concurrent document changes and reads
    for i in 0..30 {
        let service = Arc::clone(&service_arc);
        let uri = uri.clone();

        let task = tokio::spawn(async move {
            if i % 3 == 0 {
                // Document change
                let new_text = format!(
                    r#"
                    fragment UserFields on User {{
                        id
                        username
                        email{}
                    }}

                    query {{
                        users {{
                            ...UserFields
                        }}
                    }}
                    "#,
                    i
                );
                let params = DidChangeTextDocumentParams {
                    text_document: VersionedTextDocumentIdentifier {
                        uri: uri.clone(),
                        version: (i + 2),
                    },
                    content_changes: vec![TextDocumentContentChangeEvent {
                        range: None,
                        range_length: None,
                        text: new_text,
                    }],
                };
                let mut svc = service.lock().await;
                svc.call(
                    tower_lsp_server::jsonrpc::Request::build("textDocument/didChange")
                        .params(serde_json::to_value(&params).unwrap())
                        .finish(),
                )
                .await
            } else if i % 3 == 1 {
                // Completion during changes
                let params = CompletionParams {
                    text_document_position: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri },
                        position: pos(3, 20),
                    },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                    context: None,
                };
                let mut svc = service.lock().await;
                lsp_request_typed::<CompletionResponse, _>(
                    &mut svc,
                    "textDocument/completion",
                    &params,
                )
                .await;
                Ok(None)
            } else {
                // Hover during changes
                let params = HoverParams {
                    text_document_position_params: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri },
                        position: pos(3, 20),
                    },
                    work_done_progress_params: Default::default(),
                };
                let mut svc = service.lock().await;
                lsp_request_typed::<Option<Hover>, _>(&mut svc, "textDocument/hover", &params)
                    .await;
                Ok(None)
            }
        });
        tasks.push(task);
    }

    // Wait for all operations
    let mut success_count = 0;
    for task in tasks {
        if task.await.unwrap().is_ok() {
            success_count += 1;
        }
    }

    println!(
        "Completed {}/30 concurrent change operations",
        success_count
    );
    assert_eq!(
        success_count, 30,
        "All operations should complete without deadlock"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_concurrent_cross_file_references() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Create a fragment file
    let fragment_text = r#"
        fragment UserFields on User {
            id
            username
            email
        }

        fragment PostFields on Post {
            id
            title
            content
        }
    "#;
    let fragment_uri = write_project_file_at(dir.path(), "fragments.graphql", fragment_text);
    lsp_did_open(
        &mut service,
        fragment_uri.clone(),
        "graphql",
        1,
        fragment_text,
    )
    .await;

    // Create multiple query files that use the fragments
    let mut query_uris: Vec<(Uri, String)> = Vec::new();
    for i in 0..15 {
        let query_text = r#"
            query GetData {
                users {
                    ...UserFields
                }
                posts {
                    ...PostFields
                }
            }
        "#;
        let uri = write_project_file_at(dir.path(), &format!("query_{}.graphql", i), query_text);
        query_uris.push((uri, query_text.to_string()));
    }

    // Open all query files
    for (uri, text) in &query_uris {
        lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;
    }

    // Allow workspace scan to complete
    sleep(Duration::from_millis(10)).await;

    let service_arc = Arc::new(tokio::sync::Mutex::new(service));
    let mut tasks = Vec::new();

    // Concurrently request references for fragments from multiple files
    for (uri, _) in query_uris.iter() {
        // Find references to UserFields
        let service1 = Arc::clone(&service_arc);
        let uri1 = uri.clone();
        tasks.push(tokio::spawn(async move {
            let params = ReferenceParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri1 },
                    position: pos(4, 25), // Over UserFields spread
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: ReferenceContext {
                    include_declaration: true,
                },
            };
            let mut svc = service1.lock().await;
            lsp_request_typed::<Option<Vec<Location>>, _>(
                &mut svc,
                "textDocument/references",
                &params,
            )
            .await;
            Ok::<(), tower_lsp_server::jsonrpc::Error>(())
        }));

        // Also request definition
        let service2 = Arc::clone(&service_arc);
        let uri2 = uri.clone();
        tasks.push(tokio::spawn(async move {
            let params = GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri2 },
                    position: pos(4, 25),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            let mut svc = service2.lock().await;
            lsp_request_typed::<Option<GotoDefinitionResponse>, _>(
                &mut svc,
                "textDocument/definition",
                &params,
            )
            .await;
            Ok::<(), tower_lsp_server::jsonrpc::Error>(())
        }));
    }

    println!("Executing {} concurrent cross-file operations", tasks.len());

    // Wait for all operations
    let mut success_count = 0;
    for task in tasks {
        if task.await.unwrap().is_ok() {
            success_count += 1;
        }
    }

    println!(
        "Completed {}/{} cross-file operations",
        success_count,
        query_uris.len() * 2
    );
    assert_eq!(
        success_count,
        query_uris.len() * 2,
        "All cross-file operations should succeed"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_high_volume_concurrent_requests() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = r#"
        query {
            users {
                id
                username
                email
                posts {
                    id
                    title
                }
            }
        }
    "#;
    let uri = write_project_file_at(dir.path(), "test.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    sleep(Duration::from_millis(10)).await;

    let service_arc = Arc::new(tokio::sync::Mutex::new(service));
    let request_count = 100;
    let mut tasks = Vec::new();

    println!("Firing {} concurrent requests", request_count);

    // Fire a very high volume of concurrent requests
    for i in 0..request_count {
        let service = Arc::clone(&service_arc);
        let uri = uri.clone();

        let task = tokio::spawn(async move {
            let operation = i % 4;
            let mut svc = service.lock().await;
            match operation {
                0 => {
                    // Hover
                    let params = HoverParams {
                        text_document_position_params: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier { uri },
                            position: pos(4, 20),
                        },
                        work_done_progress_params: Default::default(),
                    };
                    lsp_request_typed::<Option<Hover>, _>(&mut svc, "textDocument/hover", &params)
                        .await;
                }
                1 => {
                    // Completion
                    let params = CompletionParams {
                        text_document_position: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier { uri },
                            position: pos(4, 20),
                        },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                        context: None,
                    };
                    lsp_request_typed::<CompletionResponse, _>(
                        &mut svc,
                        "textDocument/completion",
                        &params,
                    )
                    .await;
                }
                2 => {
                    // Document symbols
                    let params = DocumentSymbolParams {
                        text_document: TextDocumentIdentifier { uri },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    };
                    lsp_request_typed::<Option<DocumentSymbolResponse>, _>(
                        &mut svc,
                        "textDocument/documentSymbol",
                        &params,
                    )
                    .await;
                }
                _ => {
                    // Semantic tokens
                    let params = SemanticTokensParams {
                        text_document: TextDocumentIdentifier { uri },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    };
                    lsp_request_typed::<Option<SemanticTokensResult>, _>(
                        &mut svc,
                        "textDocument/semanticTokens/full",
                        &params,
                    )
                    .await;
                }
            }
            Ok::<(), tower_lsp_server::jsonrpc::Error>(())
        });
        tasks.push(task);
    }

    // Wait for all requests
    let start = std::time::Instant::now();
    let mut success_count = 0;
    let mut error_count = 0;

    for task in tasks {
        match task.await.unwrap() {
            Ok(_) => success_count += 1,
            Err(e) => {
                error_count += 1;
                eprintln!("Request error: {:?}", e);
            }
        }
    }

    let elapsed = start.elapsed();
    println!(
        "Completed {} requests in {:?} ({} successful, {} errors)",
        request_count, elapsed, success_count, error_count
    );
    println!(
        "Average: {:?} per request",
        elapsed / (request_count as u32)
    );

    assert!(
        error_count == 0,
        "No requests should fail due to lock contention"
    );
    assert_eq!(success_count, request_count, "All requests should succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_concurrent_100_hover_requests() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let large_schema = r#"
        type Query {
            users: [User!]!
            user(id: ID!): User
            posts: [Post!]!
            post(id: ID!): Post
            comments: [Comment!]!
            items: [Item!]!
        }

        type User {
            id: ID!
            username: String!
            email: String!
            posts: [Post!]!
            comments: [Comment!]!
        }

        type Post {
            id: ID!
            title: String!
            content: String!
            author: User!
            comments: [Comment!]!
        }

        type Comment {
            id: ID!
            text: String!
            author: User!
            post: Post!
        }

        type Item {
            id: ID!
            name: String!
            value: String!
        }
    "#;

    let uri = write_project_file_at(dir.path(), "schema.graphql", large_schema);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, large_schema).await;

    let mut uris = Vec::new();
    for i in 0..100 {
        let query_text = r#"
            query GetData {
                users {
                    id
                    username
                    email
                }
                posts {
                    id
                    title
                }
            }
            "#
        .to_string();
        let query_uri =
            write_project_file_at(dir.path(), &format!("query_{}.graphql", i), &query_text);
        lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;
        uris.push(query_uri);
    }

    sleep(Duration::from_millis(50)).await;

    let service_arc = std::sync::Arc::new(tokio::sync::Mutex::new(service));
    let start = std::time::Instant::now();

    let futures: Vec<_> = uris
        .iter()
        .map(|uri| {
            let service = std::sync::Arc::clone(&service_arc);
            let uri = uri.clone();
            tokio::spawn(async move {
                let mut svc = service.lock().await;
                let params = HoverParams {
                    text_document_position_params: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri },
                        position: pos(4, 20),
                    },
                    work_done_progress_params: Default::default(),
                };
                lsp_request_typed::<Option<Hover>, _>(&mut svc, "textDocument/hover", &params)
                    .await;
                Ok::<(), tower_lsp_server::jsonrpc::Error>(())
            })
        })
        .collect();

    let results: Vec<Result<Result<(), tower_lsp_server::jsonrpc::Error>, _>> =
        futures_util::future::join_all(futures).await;
    let duration = start.elapsed();

    let success_count = results
        .iter()
        .filter(|r| r.as_ref().is_ok_and(|r| r.is_ok()))
        .count();
    println!("100 concurrent hover requests completed in {:?}", duration);
    println!("Success: {}/100", success_count);

    assert!(
        duration < std::time::Duration::from_secs(30),
        "100 hover requests took too long: {:?}",
        duration
    );
    assert_eq!(success_count, 100, "All 100 hover requests should succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_concurrent_mixed_large() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let mut all_uris = Vec::new();
    let file_count = 50;

    for i in 0..file_count {
        let text = format!(
            r#"
            fragment UserFields{} on User {{
                id
                username
                email
            }}

            fragment PostFields{} on Post {{
                id
                title
                content
                author {{
                    id
                    username
                }}
            }}

            query GetPosts{} {{
                posts {{
                    ...PostFields{}
                    ...UserFields{}
                }}
                users {{
                    ...UserFields{}
                }}
            }}
            "#,
            i, i, i, i, i, i
        );
        let uri = write_project_file_at(dir.path(), &format!("file_{}.graphql", i), &text);
        lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;
        all_uris.push((uri, text));
    }

    sleep(Duration::from_millis(100)).await;

    let service_arc = std::sync::Arc::new(tokio::sync::Mutex::new(service));
    let mut tasks = Vec::new();
    let request_count = 200;

    println!(
        "Executing {} concurrent mixed operations across {} files",
        request_count, file_count
    );

    for i in 0..request_count {
        let (uri, _) = &all_uris[i % file_count];
        let service = std::sync::Arc::clone(&service_arc);
        let uri = uri.clone();

        let operation_type = i % 5;
        let task = tokio::spawn(async move {
            let mut svc = service.lock().await;
            match operation_type {
                0 => {
                    let params = HoverParams {
                        text_document_position_params: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier { uri },
                            position: pos(3, 20),
                        },
                        work_done_progress_params: Default::default(),
                    };
                    lsp_request_typed::<Option<Hover>, _>(&mut svc, "textDocument/hover", &params)
                        .await;
                }
                1 => {
                    let params = CompletionParams {
                        text_document_position: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier { uri },
                            position: pos(3, 20),
                        },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                        context: None,
                    };
                    lsp_request_typed::<CompletionResponse, _>(
                        &mut svc,
                        "textDocument/completion",
                        &params,
                    )
                    .await;
                }
                2 => {
                    let params = ReferenceParams {
                        text_document_position: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier { uri },
                            position: pos(2, 25),
                        },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                        context: ReferenceContext {
                            include_declaration: true,
                        },
                    };
                    lsp_request_typed::<Option<Vec<Location>>, _>(
                        &mut svc,
                        "textDocument/references",
                        &params,
                    )
                    .await;
                }
                3 => {
                    let params = GotoDefinitionParams {
                        text_document_position_params: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier { uri },
                            position: pos(12, 25),
                        },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    };
                    lsp_request_typed::<Option<GotoDefinitionResponse>, _>(
                        &mut svc,
                        "textDocument/definition",
                        &params,
                    )
                    .await;
                }
                _ => {
                    let params = DocumentSymbolParams {
                        text_document: TextDocumentIdentifier { uri },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    };
                    lsp_request_typed::<Option<DocumentSymbolResponse>, _>(
                        &mut svc,
                        "textDocument/documentSymbol",
                        &params,
                    )
                    .await;
                }
            }
            Ok::<(), tower_lsp_server::jsonrpc::Error>(())
        });
        tasks.push(task);
    }

    let start = std::time::Instant::now();
    let mut success_count = 0;
    let mut error_count = 0;

    for task in tasks {
        match task.await.unwrap() {
            Ok(_) => success_count += 1,
            Err(e) => {
                error_count += 1;
                eprintln!("Request failed: {:?}", e);
            }
        }
    }

    let elapsed = start.elapsed();
    println!(
        "Completed {} mixed operations in {:?} ({} successful, {} errors)",
        request_count, elapsed, success_count, error_count
    );
    println!(
        "Average: {:?} per request",
        elapsed / (request_count as u32)
    );

    assert!(
        error_count == 0,
        "No requests should fail due to lock contention or race conditions"
    );
    assert_eq!(success_count, request_count, "All requests should succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_concurrent_document_changes_rapid() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let initial_text = r#"
        fragment UserFields on User {
            id
            username
            email
        }

        query GetData {
            users {
                ...UserFields
            }
        }
    "#;
    let uri = write_project_file_at(dir.path(), "test.graphql", initial_text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, initial_text).await;

    sleep(Duration::from_millis(20)).await;

    let service_arc = std::sync::Arc::new(tokio::sync::Mutex::new(service));
    let mut tasks = Vec::new();
    let change_count = 50;

    for i in 0..change_count {
        let service = std::sync::Arc::clone(&service_arc);
        let uri = uri.clone();

        let task = tokio::spawn(async move {
            let mut svc = service.lock().await;

            let version = i + 2;
            let new_text = format!(
                r#"
                fragment UserFields on User {{
                    id
                    username
                    email
                    posts{} {{
                        id
                        title
                    }}
                }}

                query GetData {{
                    users {{
                        ...UserFields
                    }}
                }}
                "#,
                i
            );

            let change_params = DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier {
                    uri: uri.clone(),
                    version,
                },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: new_text.clone(),
                }],
            };

            svc.call(
                tower_lsp_server::jsonrpc::Request::build("textDocument/didChange")
                    .params(serde_json::to_value(&change_params).unwrap())
                    .finish(),
            )
            .await
        });
        tasks.push(task);
    }

    let start = std::time::Instant::now();

    for task in tasks {
        task.await.unwrap().unwrap();
    }

    let elapsed = start.elapsed();
    println!(
        "Completed {} rapid document changes in {:?}",
        change_count, elapsed
    );

    assert!(
        elapsed < std::time::Duration::from_secs(30),
        "Rapid document changes took too long: {:?}",
        elapsed
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_concurrent_cache_access() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_text = r#"
        type Query {
            users: [User!]!
            posts: [Post!]!
            comments: [Comment!]!
        }

        type User {
            id: ID!
            username: String!
            email: String!
        }

        type Post {
            id: ID!
            title: String!
            content: String!
            author: User!
        }

        type Comment {
            id: ID!
            text: String!
            post: Post!
            author: User!
        }
    "#;
    let schema_uri = write_project_file_at(dir.path(), "schema.graphql", schema_text);
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, schema_text).await;

    for i in 0..30 {
        let query_text = format!(
            r#"
            query GetData{} {{
                users {{
                    id
                    username
                    posts {{
                        id
                        title
                    }}
                }}
            }}
            "#,
            i
        );
        let uri = write_project_file_at(dir.path(), &format!("query_{}.graphql", i), &query_text);
        lsp_did_open(&mut service, uri.clone(), "graphql", 1, &query_text).await;
    }

    sleep(Duration::from_millis(50)).await;

    let service_arc = std::sync::Arc::new(tokio::sync::Mutex::new(service));
    let mut tasks = Vec::new();
    let request_count = 100;

    for _ in 0..request_count {
        let service = std::sync::Arc::clone(&service_arc);
        let uri = schema_uri.clone();

        let task = tokio::spawn(async move {
            let mut svc = service.lock().await;

            let params = HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri },
                    position: pos(3, 20),
                },
                work_done_progress_params: Default::default(),
            };

            lsp_request_typed::<Option<Hover>, _>(&mut svc, "textDocument/hover", &params).await;
            Ok::<(), tower_lsp_server::jsonrpc::Error>(())
        });
        tasks.push(task);
    }

    let start = std::time::Instant::now();
    let results: Vec<Result<Result<(), tower_lsp_server::jsonrpc::Error>, _>> =
        futures_util::future::join_all(tasks).await;
    let elapsed = start.elapsed();

    let success_count = results
        .iter()
        .filter(|r| r.as_ref().is_ok_and(|r| r.is_ok()))
        .count();
    println!(
        "{} concurrent cache accesses completed in {:?}",
        request_count, elapsed
    );
    println!("Success: {}/{}", success_count, request_count);

    assert!(
        elapsed < std::time::Duration::from_secs(20),
        "Concurrent cache accesses took too long: {:?}",
        elapsed
    );
    assert_eq!(
        success_count, request_count,
        "All cache accesses should succeed"
    );
}
