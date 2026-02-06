use apollo_compiler::Schema;
use graphql_rust::DocumentState;
use std::sync::OnceLock;
use tokio::time::{Duration, sleep};
use tower_lsp::lsp_types::*;

static SCHEMA: OnceLock<Schema> = OnceLock::new();
static VALID_SCHEMA: OnceLock<apollo_compiler::validation::Valid<Schema>> = OnceLock::new();

fn get_schema() -> &'static Schema {
    SCHEMA.get_or_init(|| {
        let schema_content = r#"
            type User {
                id: ID!
                username: String!
                email: String!
            }
            type Query {
                user(id: ID): User
            }
        "#;
        Schema::parse(schema_content, "schema.graphql").expect("Failed to parse schema")
    })
}

fn get_valid_schema() -> &'static apollo_compiler::validation::Valid<Schema> {
    VALID_SCHEMA.get_or_init(|| {
        get_schema()
            .clone()
            .validate()
            .expect("Schema validation failed")
    })
}

fn create_doc(uri_str: &str, text: &str) -> DocumentState {
    let uri = Url::parse(uri_str).unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();
    DocumentState::new(uri, text, parser)
}

#[test]
fn test_variable_used_in_fragment_spread() {
    let schema = get_valid_schema();

    let query_text = r#"
        query GetUser($id: ID, $admin: Boolean) {
            user(id: $id) {
                ...UserFields
            }
        }
        
        fragment UserFields on User {
            id
            username @include(if: $admin)
        }
    "#;

    let doc = create_doc("file:///test.graphql", query_text);
    let diagnostics = doc.get_semantic_diagnostics(schema, &[], None, None, false, true);

    let unused_vars: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("Unused variable"))
        .collect();

    assert!(
        unused_vars.is_empty(),
        "Expected no unused variables, but found: {:?}",
        unused_vars
    );
}

#[test]
fn test_variable_used_transitively_in_nested_fragments() {
    let schema = get_valid_schema();

    let query_text = r#"
        query GetUser($id: ID, $admin: Boolean) {
            user(id: $id) {
                ...Level1
            }
        }
        
        fragment Level1 on User {
            ...Level2
        }
        
        fragment Level2 on User {
            username @include(if: $admin)
        }
    "#;

    let doc = create_doc("file:///test.graphql", query_text);
    let diagnostics = doc.get_semantic_diagnostics(schema, &[], None, None, false, true);

    let unused_vars: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("Unused variable"))
        .collect();

    assert!(
        unused_vars.is_empty(),
        "Expected no unused variables in transitive case, but found: {:?}",
        unused_vars
    );
}

#[test]
fn test_variable_unused_even_with_fragments() {
    let schema = get_valid_schema();

    let query_text = r#"
        query GetUser($id: ID, $unused: String) {
            user(id: $id) {
                ...UserFields
            }
        }
        
        fragment UserFields on User {
            id
            username
        }
    "#;

    let doc = create_doc("file:///test.graphql", query_text);
    let diagnostics = doc.get_semantic_diagnostics(schema, &[], None, None, false, true);

    let unused_vars: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("Unused variable: $unused"))
        .collect();

    assert_eq!(
        unused_vars.len(),
        1,
        "Expected one unused variable ($unused)"
    );
}

#[test]
fn test_undefined_variable_direct() {
    let schema = get_valid_schema();

    let query_text = r#"
        query GetUser($id: ID) {
            user(id: $undefined) {
                id
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", query_text);
    let diagnostics = doc.get_semantic_diagnostics(schema, &[], None, None, false, true);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("Undefined variable: $undefined"))
        .collect();

    assert_eq!(errors.len(), 1, "Expected one undefined variable error");
}

#[test]
fn test_undefined_variable_in_fragment_spread() {
    let schema = get_valid_schema();

    let query_text = r#"
        query GetUser($id: ID) {
            user(id: $id) {
                ...UserFields
            }
        }
        
        fragment UserFields on User {
            username @include(if: $admin)
        }
    "#;

    let doc = create_doc("file:///test.graphql", query_text);
    let diagnostics = doc.get_semantic_diagnostics(schema, &[], None, None, false, true);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("Undefined variable: $admin"))
        .collect();

    assert_eq!(
        errors.len(),
        1,
        "Expected one undefined variable error from fragment usage"
    );
}

#[tokio::test]
async fn test_fragment_hover_requirements() {
    use graphql_rust::config::{GlobPattern, ProjectConfig, SchemaSource};
    use graphql_rust::{Backend, Config};
    use std::fs;
    use tempfile::tempdir;
    use tower_lsp::LspService;
    use tower_service::Service;

    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String friend(id: ID): User } type Query { me: User }",
    )
    .unwrap();

    let frag_path = base_dir.join("frag.graphql");
    let frag_text = "fragment UserFields on User { friend(id: $friendId) { name } }";
    fs::write(&frag_path, frag_text).unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { ...UserFields } }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        timeouts: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    service
        .call(
            tower_lsp::jsonrpc::Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap();
    service
        .call(
            tower_lsp::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    sleep(Duration::from_millis(10)).await;

    let query_uri = Url::from_file_path(query_path).unwrap();

    service
        .call(
            tower_lsp::jsonrpc::Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: query_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: query_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Hover over ...UserFields
    // query { me { ...UserFields } }
    // 0123456789012345678
    let params = HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: Position::new(0, 18),
        },
        work_done_progress_params: Default::default(),
    };

    let request = tower_lsp::jsonrpc::Request::build("textDocument/hover")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<Hover> = serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let hover = result.expect("Expected hover");
    let value = match hover.contents {
        HoverContents::Markup(m) => m.value,
        _ => panic!("Expected markup content"),
    };

    assert!(
        value.contains("**Requires Variables:**"),
        "Hover should contain requirements header"
    );
    assert!(
        value.contains("$friendId"),
        "Hover should contain $friendId"
    );
    assert!(value.contains("ID"), "Hover should contain ID");
}

#[tokio::test]
async fn test_fragment_completion_requirements() {
    use graphql_rust::config::{GlobPattern, ProjectConfig, SchemaSource};
    use graphql_rust::{Backend, Config};
    use std::fs;
    use tempfile::tempdir;
    use tower_lsp::LspService;
    use tower_service::Service;

    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID! name: String friend(id: ID): User } type Query { me: User }",
    )
    .unwrap();

    let frag_path = base_dir.join("frag.graphql");
    let frag_text = "fragment UserFields on User { friend(id: $friendId) { name } }";
    fs::write(&frag_path, frag_text).unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me { ... } }";
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        timeouts: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    service
        .call(
            tower_lsp::jsonrpc::Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap();
    service
        .call(
            tower_lsp::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    sleep(Duration::from_millis(10)).await;

    let query_uri = Url::from_file_path(query_path).unwrap();

    service
        .call(
            tower_lsp::jsonrpc::Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: query_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: query_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Completion after ...
    // query { me { ... } }
    // 01234567890123456
    let params = CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: Position::new(0, 16),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    };

    let request = tower_lsp::jsonrpc::Request::build("textDocument/completion")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CompletionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let completion = match result.expect("Expected completion") {
        CompletionResponse::Array(items) => items,
        _ => panic!("Expected array of items"),
    };

    let item = completion
        .iter()
        .find(|i| i.label == "UserFields")
        .expect("Should find UserFields completion");
    let doc = match item.documentation.as_ref().unwrap() {
        Documentation::MarkupContent(m) => &m.value,
        _ => panic!("Expected markup content"),
    };

    assert!(
        doc.contains("**Requires Variables:**"),
        "Completion doc should contain requirements header"
    );
    assert!(
        doc.contains("$friendId"),
        "Completion doc should contain $friendId"
    );
    assert!(doc.contains("ID"), "Completion doc should contain ID");
}

#[test]
fn test_variable_in_directive_requirement() {
    let schema_content = r#"
        type User { id: ID! name: String }
        type Query { me: User }
        directive @include(if: Boolean!) on FIELD | FRAGMENT_SPREAD | INLINE_FRAGMENT
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql").unwrap();

    let frag_text = "fragment UserFields on User { name @include(if: $admin) }";
    let doc = create_doc("file:///test.graphql", frag_text);

    let vars = doc.get_fragment_variable_types("UserFields", &schema);
    assert_eq!(vars.get("admin").unwrap(), "Boolean!");
}

#[tokio::test]
async fn test_variable_references_including_fragments() {
    use graphql_rust::config::{GlobPattern, ProjectConfig, SchemaSource};
    use graphql_rust::{Backend, Config};
    use std::fs;
    use tempfile::tempdir;
    use tower_lsp::LspService;
    use tower_service::Service;

    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(&schema_path, "type User { id: ID! name: String } type Query { me: User } directive @include(if: Boolean!) on FIELD | FRAGMENT_SPREAD | INLINE_FRAGMENT").unwrap();

    let frag_path = base_dir.join("frag.graphql");
    let frag_text = "fragment UserFields on User { name @include(if: $admin) }";
    fs::write(&frag_path, frag_text).unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = r#"
        query GetMe($admin: Boolean!) {
            me {
                id
                ...UserFields
            }
        }
    "#;
    fs::write(&query_path, query_text).unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.clone(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        timeouts: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    service
        .call(
            tower_lsp::jsonrpc::Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap();
    service
        .call(
            tower_lsp::jsonrpc::Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    // Wait for scan
    sleep(Duration::from_millis(10)).await;

    let query_uri = Url::from_file_path(query_path).unwrap();
    let frag_uri = Url::from_file_path(frag_path).unwrap();

    // Open files
    service
        .call(
            tower_lsp::jsonrpc::Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: query_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: query_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Request references for $admin in GetMe
    // query GetMe($admin: Boolean!)
    // 01234567890123456789
    // Position(1, 21) is on $admin
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: Position::new(1, 21),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let request = tower_lsp::jsonrpc::Request::build("textDocument/references")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<Vec<Location>> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let locations = result.expect("Expected locations");

    // We expect 2 locations:
    // 1. Declaration in query.graphql
    // 2. Usage in frag.graphql

    assert!(
        locations.iter().any(|l| l.uri == query_uri),
        "Expected reference in query.graphql"
    );
    assert!(
        locations.iter().any(|l| l.uri == frag_uri),
        "Expected reference in frag.graphql"
    );
}

#[test]
fn test_fragment_variables_not_undefined_in_isolation() {
    let schema = get_valid_schema();

    let frag_text = r#"
        fragment UserFields on User {
            id
            username @include(if: $admin)
        }
    "#;

    let doc = create_doc("file:///test.graphql", frag_text);
    let diagnostics = doc.get_semantic_diagnostics(schema, &[], None, None, false, true);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("Undefined variable: $admin"))
        .collect();

    assert!(
        errors.is_empty(),
        "Expected no undefined variable error for fragment in isolation, but found: {:?}",
        errors
    );
}

#[test]
fn test_variable_used_only_in_directive() {
    let schema_content = r#"
        type User {
            id: ID!
            name: String
        }
        type Query {
            me: User
        }
        directive @skip(if: Boolean!) on FIELD | FRAGMENT_SPREAD | INLINE_FRAGMENT
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .expect("Failed to parse schema")
        .validate()
        .expect("Schema validation failed");

    let query_text = r#"
        query GetMe($skipName: Boolean!) {
            me {
                id
                name @skip(if: $skipName)
            }
        }
    "#;

    let doc = create_doc("file:///test.graphql", query_text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    let unused_vars: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("Unused variable: $skipName"))
        .collect();

    assert!(
        unused_vars.is_empty(),
        "Expected no unused variables, but found: {:?}",
        unused_vars
    );
}
