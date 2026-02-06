use graphql_rust::{
    Backend, Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
};
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::time::{Duration, sleep};
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

/// Helper to create a test config with a schema
fn create_test_config(dir: &std::path::Path) -> Config {
    let schema_path = dir.join("schema.graphql");
    fs::write(
        &schema_path,
        r#"
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
        "#,
    )
    .unwrap();

    Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(false), // Disable for predictable behavior
        base_dir: dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn test_concurrent_document_operations() {
    let dir = tempdir().unwrap();
    let config = create_test_config(dir.path());
    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    let init_params = InitializeParams {
        ..Default::default()
    };
    let request = Request::build("initialize")
        .params(serde_json::to_value(&init_params).unwrap())
        .id(0)
        .finish();
    service.call(request).await.unwrap().unwrap();

    let request = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    service.call(request).await.unwrap();

    // Create multiple GraphQL files
    let file_count = 20;
    let mut uris = Vec::new();

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
        let uri = Url::from_file_path(&query_path).unwrap();
        uris.push((uri, text));
    }

    // Concurrently open all documents
    let service = Arc::new(tokio::sync::Mutex::new(service));
    let mut tasks = Vec::new();

    for (uri, text) in uris.clone() {
        let service = Arc::clone(&service);
        let task = tokio::spawn(async move {
            let params = DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "graphql".to_string(),
                    version: 1,
                    text: text.clone(),
                },
            };
            let request = Request::build("textDocument/didOpen")
                .params(serde_json::to_value(&params).unwrap())
                .finish();

            let mut svc = service.lock().await;
            svc.call(request).await
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

    for (i, (uri, _)) in uris.iter().enumerate() {
        let service = Arc::clone(&service);
        let uri = uri.clone();
        let request_id = i as i64 + 100;

        // Hover request
        let task = tokio::spawn(async move {
            let position = Position::new(3, 20); // Over "username"
            let params = HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
            };

            let request = Request::build("textDocument/hover")
                .id(request_id)
                .params(serde_json::to_value(&params).unwrap())
                .finish();

            let mut svc = service.lock().await;
            svc.call(request).await
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
    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    let init_params = InitializeParams {
        ..Default::default()
    };
    let request = Request::build("initialize")
        .params(serde_json::to_value(&init_params).unwrap())
        .id(0)
        .finish();
    service.call(request).await.unwrap().unwrap();

    let request = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    service.call(request).await.unwrap();

    // Create a test file
    let query_path = dir.path().join("test.graphql");
    let text = "query { users {  } }";
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    let request = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    service.call(request).await.unwrap();

    // Allow a brief moment for async processing
    sleep(Duration::from_millis(10)).await;

    let service = Arc::new(tokio::sync::Mutex::new(service));
    let mut tasks = Vec::new();

    // Fire 50 concurrent completion requests at the same position
    for i in 0..50 {
        let service = Arc::clone(&service);
        let uri = uri.clone();

        let task = tokio::spawn(async move {
            let position = Position::new(0, 16); // Inside "users { | }"
            let params = CompletionParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: None,
            };

            let request = Request::build("textDocument/completion")
                .id(i + 1000)
                .params(serde_json::to_value(&params).unwrap())
                .finish();

            let mut svc = service.lock().await;
            svc.call(request).await
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
    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    let init_params = InitializeParams {
        ..Default::default()
    };
    let request = Request::build("initialize")
        .params(serde_json::to_value(&init_params).unwrap())
        .id(0)
        .finish();
    service.call(request).await.unwrap().unwrap();

    let request = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    service.call(request).await.unwrap();

    // Create multiple files
    let file_count = 10;
    let mut uris = Vec::new();

    for i in 0..file_count {
        let query_path = dir.path().join(format!("file_{}.graphql", i));
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
        fs::write(&query_path, &text).unwrap();
        let query_path = std::fs::canonicalize(query_path).unwrap();
        let uri = Url::from_file_path(&query_path).unwrap();
        uris.push((uri, text));
    }

    // Open all documents
    for (uri, text) in &uris {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "graphql".to_string(),
                version: 1,
                text: text.clone(),
            },
        };
        let request = Request::build("textDocument/didOpen")
            .params(serde_json::to_value(&params).unwrap())
            .finish();
        service.call(request).await.unwrap();
    }

    // Allow a brief moment for async processing
    sleep(Duration::from_millis(10)).await;

    let service = Arc::new(tokio::sync::Mutex::new(service));
    let mut tasks = Vec::new();
    let mut request_id = 2000;

    // Mix different types of concurrent requests
    for (i, (uri, _)) in uris.iter().enumerate() {
        let service = Arc::clone(&service);
        let uri = uri.clone();

        // Hover
        let hover_task = {
            let service = Arc::clone(&service);
            let uri = uri.clone();
            let id = request_id;
            request_id += 1;
            tokio::spawn(async move {
                let position = Position::new(3, 20);
                let params = HoverParams {
                    text_document_position_params: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri },
                        position,
                    },
                    work_done_progress_params: Default::default(),
                };
                let request = Request::build("textDocument/hover")
                    .id(id)
                    .params(serde_json::to_value(&params).unwrap())
                    .finish();
                let mut svc = service.lock().await;
                svc.call(request).await
            })
        };
        tasks.push(hover_task);

        // Completion
        let completion_task = {
            let service = Arc::clone(&service);
            let uri = uri.clone();
            let id = request_id;
            request_id += 1;
            tokio::spawn(async move {
                let position = Position::new(3, 20);
                let params = CompletionParams {
                    text_document_position: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri },
                        position,
                    },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                    context: None,
                };
                let request = Request::build("textDocument/completion")
                    .id(id)
                    .params(serde_json::to_value(&params).unwrap())
                    .finish();
                let mut svc = service.lock().await;
                svc.call(request).await
            })
        };
        tasks.push(completion_task);

        // Definition
        let definition_task = {
            let service = Arc::clone(&service);
            let uri = uri.clone();
            let id = request_id;
            request_id += 1;
            tokio::spawn(async move {
                let position = Position::new(10, 25); // Over fragment spread
                let params = GotoDefinitionParams {
                    text_document_position_params: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri },
                        position,
                    },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                };
                let request = Request::build("textDocument/definition")
                    .id(id)
                    .params(serde_json::to_value(&params).unwrap())
                    .finish();
                let mut svc = service.lock().await;
                svc.call(request).await
            })
        };
        tasks.push(definition_task);

        // References
        let references_task = {
            let service = Arc::clone(&service);
            let uri = uri.clone();
            let id = request_id;
            request_id += 1;
            tokio::spawn(async move {
                let position = Position::new(2, 25); // Over fragment name
                let params = ReferenceParams {
                    text_document_position: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri },
                        position,
                    },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                    context: ReferenceContext {
                        include_declaration: true,
                    },
                };
                let request = Request::build("textDocument/references")
                    .id(id)
                    .params(serde_json::to_value(&params).unwrap())
                    .finish();
                let mut svc = service.lock().await;
                svc.call(request).await
            })
        };
        tasks.push(references_task);

        // Document symbols (every other file)
        if i % 2 == 0 {
            let symbols_task = {
                let service = Arc::clone(&service);
                let uri = uri.clone();
                let id = request_id;
                request_id += 1;
                tokio::spawn(async move {
                    let params = DocumentSymbolParams {
                        text_document: TextDocumentIdentifier { uri },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    };
                    let request = Request::build("textDocument/documentSymbol")
                        .id(id)
                        .params(serde_json::to_value(&params).unwrap())
                        .finish();
                    let mut svc = service.lock().await;
                    svc.call(request).await
                })
            };
            tasks.push(symbols_task);
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
    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    let init_params = InitializeParams {
        ..Default::default()
    };
    let request = Request::build("initialize")
        .params(serde_json::to_value(&init_params).unwrap())
        .id(0)
        .finish();
    service.call(request).await.unwrap().unwrap();

    let request = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    service.call(request).await.unwrap();

    let query_path = dir.path().join("test.graphql");
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
    fs::write(&query_path, initial_text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: initial_text.to_string(),
        },
    };
    let request = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    service.call(request).await.unwrap();

    let service = Arc::new(tokio::sync::Mutex::new(service));
    let mut tasks = Vec::new();

    // Simulate rapid concurrent document changes and reads
    for i in 0..30 {
        let service = Arc::clone(&service);
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
                        version: (i + 2) as i32,
                    },
                    content_changes: vec![TextDocumentContentChangeEvent {
                        range: None,
                        range_length: None,
                        text: new_text,
                    }],
                };
                let request = Request::build("textDocument/didChange")
                    .params(serde_json::to_value(&params).unwrap())
                    .finish();
                let mut svc = service.lock().await;
                svc.call(request).await
            } else if i % 3 == 1 {
                // Completion during changes
                let position = Position::new(3, 20);
                let params = CompletionParams {
                    text_document_position: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri },
                        position,
                    },
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                    context: None,
                };
                let request = Request::build("textDocument/completion")
                    .id(i + 3000)
                    .params(serde_json::to_value(&params).unwrap())
                    .finish();
                let mut svc = service.lock().await;
                svc.call(request).await
            } else {
                // Hover during changes
                let position = Position::new(3, 20);
                let params = HoverParams {
                    text_document_position_params: TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri },
                        position,
                    },
                    work_done_progress_params: Default::default(),
                };
                let request = Request::build("textDocument/hover")
                    .id(i + 3000)
                    .params(serde_json::to_value(&params).unwrap())
                    .finish();
                let mut svc = service.lock().await;
                svc.call(request).await
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
    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    let init_params = InitializeParams {
        ..Default::default()
    };
    let request = Request::build("initialize")
        .params(serde_json::to_value(&init_params).unwrap())
        .id(0)
        .finish();
    service.call(request).await.unwrap().unwrap();

    let request = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    service.call(request).await.unwrap();

    // Create a fragment file
    let fragment_path = dir.path().join("fragments.graphql");
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
    fs::write(&fragment_path, fragment_text).unwrap();
    let fragment_path = std::fs::canonicalize(fragment_path).unwrap();
    let fragment_uri = Url::from_file_path(&fragment_path).unwrap();

    // Create multiple query files that use the fragments
    let mut query_uris = Vec::new();
    for i in 0..15 {
        let query_path = dir.path().join(format!("query_{}.graphql", i));
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
        fs::write(&query_path, query_text).unwrap();
        let query_path = std::fs::canonicalize(query_path).unwrap();
        let uri = Url::from_file_path(&query_path).unwrap();
        query_uris.push((uri, query_text.to_string()));
    }

    // Open fragment file
    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: fragment_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: fragment_text.to_string(),
        },
    };
    let request = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    service.call(request).await.unwrap();

    // Open all query files
    for (uri, text) in &query_uris {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "graphql".to_string(),
                version: 1,
                text: text.clone(),
            },
        };
        let request = Request::build("textDocument/didOpen")
            .params(serde_json::to_value(&params).unwrap())
            .finish();
        service.call(request).await.unwrap();
    }

    // Allow workspace scan to complete
    sleep(Duration::from_millis(10)).await;

    let service = Arc::new(tokio::sync::Mutex::new(service));
    let mut tasks = Vec::new();

    // Concurrently request references for fragments from multiple files
    for (i, (uri, _)) in query_uris.iter().enumerate() {
        // Find references to UserFields
        let service1 = Arc::clone(&service);
        let uri1 = uri.clone();
        let task = tokio::spawn(async move {
            let position = Position::new(4, 25); // Over UserFields spread
            let params = ReferenceParams {
                text_document_position: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri1 },
                    position,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
                context: ReferenceContext {
                    include_declaration: true,
                },
            };
            let request = Request::build("textDocument/references")
                .id((i as i64) + 4000)
                .params(serde_json::to_value(&params).unwrap())
                .finish();
            let mut svc = service1.lock().await;
            svc.call(request).await
        });
        tasks.push(task);

        // Also request definition
        let service2 = Arc::clone(&service);
        let uri2 = uri.clone();
        let def_task = tokio::spawn(async move {
            let position = Position::new(4, 25);
            let params = GotoDefinitionParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri2 },
                    position,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            let request = Request::build("textDocument/definition")
                .id((i as i64) + 5000)
                .params(serde_json::to_value(&params).unwrap())
                .finish();
            let mut svc = service2.lock().await;
            svc.call(request).await
        });
        tasks.push(def_task);
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
    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    let init_params = InitializeParams {
        ..Default::default()
    };
    let request = Request::build("initialize")
        .params(serde_json::to_value(&init_params).unwrap())
        .id(0)
        .finish();
    service.call(request).await.unwrap().unwrap();

    let request = Request::build("initialized")
        .params(serde_json::json!({}))
        .finish();
    service.call(request).await.unwrap();

    let query_path = dir.path().join("test.graphql");
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
    fs::write(&query_path, text).unwrap();
    let query_path = std::fs::canonicalize(query_path).unwrap();
    let uri = Url::from_file_path(&query_path).unwrap();

    let params = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: text.to_string(),
        },
    };
    let request = Request::build("textDocument/didOpen")
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    service.call(request).await.unwrap();

    sleep(Duration::from_millis(10)).await;

    let service = Arc::new(tokio::sync::Mutex::new(service));
    let request_count = 100;
    let mut tasks = Vec::new();

    println!("Firing {} concurrent requests", request_count);

    // Fire a very high volume of concurrent requests
    for i in 0..request_count {
        let service = Arc::clone(&service);
        let uri = uri.clone();

        let task = tokio::spawn(async move {
            let operation = i % 4;
            match operation {
                0 => {
                    // Hover
                    let position = Position::new(4, 20);
                    let params = HoverParams {
                        text_document_position_params: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier { uri },
                            position,
                        },
                        work_done_progress_params: Default::default(),
                    };
                    let request = Request::build("textDocument/hover")
                        .id(i + 6000)
                        .params(serde_json::to_value(&params).unwrap())
                        .finish();
                    let mut svc = service.lock().await;
                    svc.call(request).await
                }
                1 => {
                    // Completion
                    let position = Position::new(4, 20);
                    let params = CompletionParams {
                        text_document_position: TextDocumentPositionParams {
                            text_document: TextDocumentIdentifier { uri },
                            position,
                        },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                        context: None,
                    };
                    let request = Request::build("textDocument/completion")
                        .id(i + 6000)
                        .params(serde_json::to_value(&params).unwrap())
                        .finish();
                    let mut svc = service.lock().await;
                    svc.call(request).await
                }
                2 => {
                    // Document symbols
                    let params = DocumentSymbolParams {
                        text_document: TextDocumentIdentifier { uri },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    };
                    let request = Request::build("textDocument/documentSymbol")
                        .id(i + 6000)
                        .params(serde_json::to_value(&params).unwrap())
                        .finish();
                    let mut svc = service.lock().await;
                    svc.call(request).await
                }
                _ => {
                    // Semantic tokens
                    let params = SemanticTokensParams {
                        text_document: TextDocumentIdentifier { uri },
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    };
                    let request = Request::build("textDocument/semanticTokens/full")
                        .id(i + 6000)
                        .params(serde_json::to_value(&params).unwrap())
                        .finish();
                    let mut svc = service.lock().await;
                    svc.call(request).await
                }
            }
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
