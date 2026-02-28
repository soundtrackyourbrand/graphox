use crate::support::{
    TestWorkspace, create_doc, create_initialized_lsp_service, lsp_did_open, lsp_request_typed,
    pos_for_token,
};
use graphox_core::config::{GlobPattern, ProjectConfig, SchemaSource};
use graphox_core::{CodegenConfig, Config};
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_goto_definition_subgraph_type() {
    let workspace = TestWorkspace::new();

    // 1. Create main schema
    let main_schema = r#"
      directive @key(fields: String!) on OBJECT | INTERFACE
      type Query {
        user(id: ID!): User
      }
      type User @key(fields: "id") {
        id: ID!
        username: String!
      }
    "#;
    let schema_uri = workspace.write_file("schema.graphql", main_schema);
    let schema_url = Url::from_file_path(std::fs::canonicalize(schema_uri).unwrap()).unwrap();

    // 2. Create subgraph schema
    let subgraph_schema = r#"
      extend type User @key(fields: "id") {
        id: ID! @external
        email: String!
      }
    "#;
    let subgraph_uri = workspace.write_file("subgraphs/user.graphql", subgraph_schema);
    let subgraph_url = Url::from_file_path(std::fs::canonicalize(subgraph_uri).unwrap()).unwrap();

    // 3. Create config with subgraphs
    let config = Config::new_test(
        workspace.root().to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_subgraphs_dir("subgraphs".to_string())
                .with_include(GlobPattern::Single("**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open main schema
    lsp_did_open(&mut service, schema_url.clone(), "graphql", 1, main_schema).await;

    // Open subgraph schema
    lsp_did_open(
        &mut service,
        subgraph_url.clone(),
        "graphql",
        1,
        subgraph_schema,
    )
    .await;

    // 4. Trigger Go to Definition on "User" in main schema
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: schema_url.clone(),
            },
            position: pos_for_token(
                &create_doc(schema_url.as_str(), main_schema),
                main_schema,
                "User",
            ),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert!(loc.uri == schema_url || loc.uri == subgraph_url);
    } else if let Some(GotoDefinitionResponse::Array(locs)) = result {
        let uris: Vec<_> = locs.iter().map(|l| l.uri.clone()).collect();
        assert!(uris.contains(&schema_url));
        assert!(uris.contains(&subgraph_url));
    } else {
        panic!("Expected definition(s) for User, got {:?}", result);
    }
}

#[tokio::test]
async fn test_workspace_symbols_includes_subgraphs() {
    let workspace = TestWorkspace::new();

    // 1. Create main schema
    let main_schema = "type Query { id: ID }";
    workspace.write_file("schema.graphql", main_schema);

    // 2. Create subgraph schema with a unique type
    let subgraph_schema = "type SubgraphType { field: String }";
    workspace.write_file("subgraphs/unique.graphql", subgraph_schema);

    // 3. Create config with subgraphs
    let config = Config::new_test(
        workspace.root().to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_subgraphs_dir("subgraphs".to_string())
                .with_include(GlobPattern::Single("**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 4. Request workspace symbols for "SubgraphType"
    let params = WorkspaceSymbolParams {
        query: "SubgraphType".to_string(),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<Vec<SymbolInformation>> =
        lsp_request_typed(&mut service, "workspace/symbol", &params).await;

    let symbols = result.expect("Expected workspace symbols");
    assert!(
        symbols.iter().any(|s| s.name == "SubgraphType"),
        "SubgraphType not found in workspace symbols. Found: {:?}",
        symbols.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
}

const SLO_DIRECTIVE_DEF: &str = r#"
  directive @slo(
    class: RequestClass
  ) on SCHEMA | FIELD_DEFINITION
  enum RequestClass { CRITICAL HIGH_FAST HIGH_SLOW LOW NO_SLO }
"#;

#[tokio::test]
async fn test_hover_slo_field_with_fallback() {
    let workspace = TestWorkspace::new();

    // 1. Create main schema
    let main_schema = r#"
      type Query {
        user: User
      }
      type User {
        id: ID!
        username: String!
        email: String!
      }
    "#;
    workspace.write_file("schema.graphql", main_schema);

    // 2. Create subgraph schema with schema SLO (Enum) and field SLO (String)
    let mut subgraph_schema = SLO_DIRECTIVE_DEF.to_string();
    subgraph_schema.push_str(
        r#"
      type Query { user: User }
      type User {
        id: ID! @slo(class: "CRITICAL")
        username: String!
      }

      schema @slo(class: HIGH_FAST) {
        query: Query
      }
    "#,
    );
    workspace.write_file("subgraphs/user.graphql", &subgraph_schema);

    // 3. Create config with subgraphs
    let config = Config::new_test(
        workspace.root().to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_subgraphs_dir("subgraphs".to_string())
                .with_include(GlobPattern::Single("**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Create a query file
    let query_text = "query Test { user { id username } }";
    let query_uri = workspace.write_file("query.graphql", query_text);
    let query_url = Url::from_file_path(std::fs::canonicalize(query_uri).unwrap()).unwrap();

    lsp_did_open(&mut service, query_url.clone(), "graphql", 1, query_text).await;

    // Wait for indexing
    let start = std::time::Instant::now();
    let mut indexed = false;
    while start.elapsed() < std::time::Duration::from_secs(5) {
        let pos_id = pos_for_token(
            &create_doc(query_url.as_str(), query_text),
            query_text,
            "id",
        );
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: query_url.clone(),
                },
                position: pos_id,
            },
            work_done_progress_params: Default::default(),
        };
        let result: Option<Hover> =
            lsp_request_typed(&mut service, "textDocument/hover", &params).await;
        if let Some(hover) = result
            && let HoverContents::Markup(markup) = hover.contents
            && markup.value.contains("Subgraphs")
        {
            indexed = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(indexed, "LSP failed to index query file in time");

    // 4. Hover over "id" (explicit SLO: CRITICAL as String)
    let pos_id = pos_for_token(
        &create_doc(query_url.as_str(), query_text),
        query_text,
        "id",
    );
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_url.clone(),
            },
            position: pos_id,
        },
        work_done_progress_params: Default::default(),
    };

    let result: Option<Hover> =
        lsp_request_typed(&mut service, "textDocument/hover", &params).await;
    let hover = result.expect("Expected hover for id");
    if let HoverContents::Markup(markup) = hover.contents {
        assert!(
            markup.value.contains("[SLO: CRITICAL]"),
            "Hover for 'id' should contain SLO: CRITICAL. Got: {}",
            markup.value
        );
    } else {
        panic!("Expected markup hover contents");
    }

    // 5. Hover over "username" (fallback SLO: HIGH_FAST from schema as Enum)
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_url.clone(),
            },
            position: pos_for_token(
                &create_doc(query_url.as_str(), query_text),
                query_text,
                "username",
            ),
        },
        work_done_progress_params: Default::default(),
    };

    let result: Option<Hover> =
        lsp_request_typed(&mut service, "textDocument/hover", &params).await;
    let hover = result.expect("Expected hover for username");
    if let HoverContents::Markup(markup) = hover.contents {
        assert!(
            markup.value.contains("[SLO: HIGH_FAST]"),
            "Hover for 'username' should contain fallback SLO: HIGH_FAST. Got: {}",
            markup.value
        );
    } else {
        panic!("Expected markup hover contents");
    }
}

#[tokio::test]
async fn test_hover_slo_operation_worst() {
    let workspace = TestWorkspace::new();

    // 1. Create main schema
    let main_schema = r#"
      type Query {
        a: String
        b: String
      }
    "#;
    workspace.write_file("schema.graphql", main_schema);

    // 2. Create subgraph schema with different SLOs
    let mut subgraph_schema = SLO_DIRECTIVE_DEF.to_string();
    subgraph_schema.push_str(
        r#"
      type Query {
        a: String @slo(class: CRITICAL)
        b: String @slo(class: "LOW")
      }
    "#,
    );
    workspace.write_file("subgraphs/api.graphql", &subgraph_schema);

    // 3. Create config with subgraphs
    let config = Config::new_test(
        workspace.root().to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_subgraphs_dir("subgraphs".to_string())
                .with_include(GlobPattern::Single("**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Create a query file
    let query_text = "query GetBoth { a b }";
    let query_uri = workspace.write_file("query.graphql", query_text);
    let query_url = Url::from_file_path(std::fs::canonicalize(query_uri).unwrap()).unwrap();

    lsp_did_open(&mut service, query_url.clone(), "graphql", 1, query_text).await;

    // Wait for indexing
    let start = std::time::Instant::now();
    let mut indexed = false;
    while start.elapsed() < std::time::Duration::from_secs(5) {
        let pos = pos_for_token(
            &create_doc(query_url.as_str(), query_text),
            query_text,
            "GetBoth",
        );
        let params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: query_url.clone(),
                },
                position: pos,
            },
            work_done_progress_params: Default::default(),
        };
        let result: Option<Hover> =
            lsp_request_typed(&mut service, "textDocument/hover", &params).await;
        if let Some(hover) = result
            && let HoverContents::Markup(markup) = hover.contents
            && markup.value.contains("**Worst SLO:** LOW")
        {
            indexed = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(indexed, "LSP failed to index query file in time");

    // 4. Hover over "GetBoth" (Worst SLO should be LOW)
    let pos = pos_for_token(
        &create_doc(query_url.as_str(), query_text),
        query_text,
        "GetBoth",
    );
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_url.clone(),
            },
            position: pos,
        },
        work_done_progress_params: Default::default(),
    };

    let result: Option<Hover> =
        lsp_request_typed(&mut service, "textDocument/hover", &params).await;
    let hover = result.expect("Expected hover for GetBoth");
    if let HoverContents::Markup(markup) = hover.contents {
        assert!(
            markup.value.contains("**Worst SLO:** LOW"),
            "Hover for 'GetBoth' should contain Worst SLO: LOW. Got: {}",
            markup.value
        );
    } else {
        panic!("Expected markup hover contents");
    }
}

#[tokio::test]
async fn test_completion_slo_field() {
    let workspace = TestWorkspace::new();

    // 1. Create main schema
    let main_schema = "type Query { a: String }";
    workspace.write_file("schema.graphql", main_schema);

    // 2. Create subgraph schema with SLO
    let mut subgraph_schema = SLO_DIRECTIVE_DEF.to_string();
    subgraph_schema.push_str("type Query { a: String @slo(class: HIGH_SLOW) }");
    workspace.write_file("subgraphs/api.graphql", &subgraph_schema);

    // 3. Create config with subgraphs
    let config = Config::new_test(
        workspace.root().to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_subgraphs_dir("subgraphs".to_string())
                .with_include(GlobPattern::Single("**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Create a query file
    let query_text = "query {  }";
    let query_uri = workspace.write_file("query.graphql", query_text);
    let query_url = Url::from_file_path(std::fs::canonicalize(query_uri).unwrap()).unwrap();

    lsp_did_open(&mut service, query_url.clone(), "graphql", 1, query_text).await;

    // Wait for indexing
    let start = std::time::Instant::now();
    let mut indexed = false;
    while start.elapsed() < std::time::Duration::from_secs(5) {
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: query_url.clone(),
                },
                position: Position::new(0, 8), // query { | }
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        };

        let result: Option<CompletionResponse> =
            lsp_request_typed(&mut service, "textDocument/completion", &params).await;

        if let Some(res) = result {
            let items = match res {
                CompletionResponse::Array(items) => items,
                CompletionResponse::List(list) => list.items,
            };
            if let Some(item_a) = items.iter().find(|i| i.label == "a") {
                let has_slo_info = item_a
                    .detail
                    .as_ref()
                    .is_some_and(|d| d.contains("[SLO: HIGH_SLOW]"))
                    || item_a.documentation.as_ref().is_some_and(|doc| {
                        if let Documentation::MarkupContent(m) = doc {
                            m.value.contains("[SLO: HIGH_SLOW]")
                        } else {
                            false
                        }
                    });
                if has_slo_info {
                    indexed = true;
                    break;
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(indexed, "LSP failed to index query file in time");

    // 4. Trigger completion inside selection set
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_url.clone(),
            },
            position: Position::new(0, 8), // query { | }
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };

    let result: Option<CompletionResponse> =
        lsp_request_typed(&mut service, "textDocument/completion", &params).await;

    let items = match result.expect("Expected completions") {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };

    let item_a = items
        .iter()
        .find(|i| i.label == "a")
        .expect("Expected completion for field 'a'");

    // Check detail or documentation for SLO info
    let has_slo_info = item_a
        .detail
        .as_ref()
        .is_some_and(|d| d.contains("[SLO: HIGH_SLOW]"))
        || item_a.documentation.as_ref().is_some_and(|doc| {
            if let Documentation::MarkupContent(m) = doc {
                m.value.contains("[SLO: HIGH_SLOW]")
            } else {
                false
            }
        });

    assert!(
        has_slo_info,
        "Field completion for 'a' should contain SLO: HIGH_SLOW. Detail: {:?}, Doc: {:?}",
        item_a.detail, item_a.documentation
    );
}

#[tokio::test]
async fn test_completion_slo_fragment_spread() {
    let workspace = TestWorkspace::new();

    // 1. Create main schema
    let main_schema = "type Query { a: String b: String }";
    workspace.write_file("schema.graphql", main_schema);

    // 2. Create subgraph schema with SLOs
    let mut subgraph_schema = SLO_DIRECTIVE_DEF.to_string();
    subgraph_schema.push_str(
        r#"
      type Query {
        a: String @slo(class: "CRITICAL")
        b: String @slo(class: NO_SLO)
      }
    "#,
    );
    workspace.write_file("subgraphs/api.graphql", &subgraph_schema);

    // 3. Create config with subgraphs
    let config = Config::new_test(
        workspace.root().to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_subgraphs_dir("subgraphs".to_string())
                .with_include(GlobPattern::Single("**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 4. Create a fragment that uses both fields
    let frag_text = "fragment Frag on Query { a b }";
    let frag_uri = workspace.write_file("fragment.graphql", frag_text);
    let frag_url = Url::from_file_path(std::fs::canonicalize(frag_uri).unwrap()).unwrap();
    lsp_did_open(&mut service, frag_url.clone(), "graphql", 1, frag_text).await;

    // 5. Create a query file and trigger completion for fragment spread
    let query_text = "query { ... }";
    let query_uri = workspace.write_file("query.graphql", query_text);
    let query_url = Url::from_file_path(std::fs::canonicalize(query_uri).unwrap()).unwrap();
    lsp_did_open(&mut service, query_url.clone(), "graphql", 1, query_text).await;

    // Wait for indexing
    let start = std::time::Instant::now();
    let mut indexed = false;
    while start.elapsed() < std::time::Duration::from_secs(5) {
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: query_url.clone(),
                },
                position: Position::new(0, 11), // query { ...| }
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        };

        let result: Option<CompletionResponse> =
            lsp_request_typed(&mut service, "textDocument/completion", &params).await;

        if let Some(res) = result {
            let items = match res {
                CompletionResponse::Array(items) => items,
                CompletionResponse::List(list) => list.items,
            };
            if let Some(item_frag) = items.iter().find(|i| i.label == "Frag") {
                let has_slo_info = item_frag
                    .detail
                    .as_ref()
                    .is_some_and(|d| d.contains("Worst SLO: NO_SLO"))
                    || item_frag.documentation.as_ref().is_some_and(|doc| {
                        if let Documentation::MarkupContent(m) = doc {
                            m.value.contains("**Worst SLO:** NO_SLO")
                        } else {
                            false
                        }
                    });
                if has_slo_info {
                    indexed = true;
                    break;
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(indexed, "LSP failed to index query file in time");

    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_url.clone(),
            },
            position: Position::new(0, 11), // query { ...| }
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };

    let result: Option<CompletionResponse> =
        lsp_request_typed(&mut service, "textDocument/completion", &params).await;

    let items = match result.expect("Expected completions") {
        CompletionResponse::Array(items) => items,
        CompletionResponse::List(list) => list.items,
    };

    let item_frag = items
        .iter()
        .find(|i| i.label == "Frag")
        .expect("Expected completion for fragment 'Frag'");

    // Worst SLO should be NO_SLO
    let has_slo_info = item_frag
        .detail
        .as_ref()
        .is_some_and(|d| d.contains("Worst SLO: NO_SLO"))
        || item_frag.documentation.as_ref().is_some_and(|doc| {
            if let Documentation::MarkupContent(m) = doc {
                m.value.contains("**Worst SLO:** NO_SLO")
            } else {
                false
            }
        });

    assert!(
        has_slo_info,
        "Fragment completion for 'Frag' should contain Worst SLO: NO_SLO. Detail: {:?}, Doc: {:?}",
        item_frag.detail, item_frag.documentation
    );
}
