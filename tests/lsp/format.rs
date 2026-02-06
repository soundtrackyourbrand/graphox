use graphql_rust::{
    Backend, Config, DocumentState,
    config::{GlobPattern, ProjectConfig, SchemaSource},
};
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

/// Helper to load fixture file content
fn load_fixture(name: &str) -> String {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/format")
        .join(name);
    fs::read_to_string(fixture_path).expect("Failed to read fixture file")
}

/// Helper to load baseline file content
fn load_baseline(name: &str) -> String {
    let baseline_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/baselines/format")
        .join(name);
    fs::read_to_string(baseline_path).expect("Failed to read baseline file")
}

/// Helper to parse a GraphQL block from a TypeScript file and format it
fn format_graphql_from_file(file_content: &str, uri_str: &str) -> Option<String> {
    let uri = Url::parse(uri_str).unwrap();
    let mut parser = tree_sitter::Parser::new();

    // Determine language from URI
    let lang = if uri_str.ends_with(".tsx") {
        tree_sitter_typescript::LANGUAGE_TSX
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT
    };

    parser.set_language(&lang.into()).unwrap();
    let doc = DocumentState::new(uri, file_content, parser);

    // Get the first GraphQL block and format it
    if let Some(block) = doc.get_graphql_trees().first() {
        let block_start = block.offset;
        let block_end = block.offset + block.tree.root_node().end_byte();
        let graphql_content = doc.rope.byte_slice(block_start..block_end).to_string();

        // Parse and format using apollo-compiler
        let mut parser = apollo_compiler::parser::Parser::new();
        let doc = parser.parse_ast(graphql_content, "inline.graphql");

        let formatted = match doc {
            Ok(document)
            | Err(apollo_compiler::validation::WithErrors {
                partial: document, ..
            }) => document.to_string(),
        };

        Some(formatted)
    } else {
        None
    }
}

#[test]
fn test_format_cramped_query() {
    let fixture = load_fixture("cramped_query.ts");
    let expected = load_baseline("cramped_query.expected.graphql");

    let formatted =
        format_graphql_from_file(&fixture, "file:///test.ts").expect("Should find GraphQL block");

    assert_eq!(
        formatted.trim(),
        expected.trim(),
        "Formatted query should match baseline"
    );
}

#[test]
fn test_format_cramped_mutation() {
    let fixture = load_fixture("cramped_mutation.tsx");
    let expected = load_baseline("cramped_mutation.expected.graphql");

    let formatted =
        format_graphql_from_file(&fixture, "file:///test.tsx").expect("Should find GraphQL block");

    assert_eq!(
        formatted.trim(),
        expected.trim(),
        "Formatted mutation should match baseline"
    );
}

#[test]
fn test_format_fragment_spread() {
    let fixture = load_fixture("fragment_spread.ts");
    let expected_fragment = load_baseline("fragment_spread_fragment.expected.graphql");
    let expected_query = load_baseline("fragment_spread_query.expected.graphql");

    let uri = Url::parse("file:///test.ts").unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .unwrap();
    let doc = DocumentState::new(uri, &fixture, parser);

    // Should have two GraphQL blocks (fragment and query)
    let blocks = doc.get_graphql_trees();
    assert_eq!(blocks.len(), 2, "Should find 2 GraphQL blocks");

    // Format first block (fragment)
    let block1 = &blocks[0];
    let block_start = block1.offset;
    let block_end = block1.offset + block1.tree.root_node().end_byte();
    let graphql_content = doc.rope.byte_slice(block_start..block_end).to_string();

    let mut parser = apollo_compiler::parser::Parser::new();
    let parsed = parser.parse_ast(graphql_content, "inline.graphql");
    let formatted1 = match parsed {
        Ok(document)
        | Err(apollo_compiler::validation::WithErrors {
            partial: document, ..
        }) => document.to_string(),
    };

    assert_eq!(
        formatted1.trim(),
        expected_fragment.trim(),
        "Formatted fragment should match baseline"
    );

    // Format second block (query)
    let block2 = &blocks[1];
    let block_start = block2.offset;
    let block_end = block2.offset + block2.tree.root_node().end_byte();
    let graphql_content = doc.rope.byte_slice(block_start..block_end).to_string();

    let mut parser = apollo_compiler::parser::Parser::new();
    let parsed = parser.parse_ast(graphql_content, "inline.graphql");
    let formatted2 = match parsed {
        Ok(document)
        | Err(apollo_compiler::validation::WithErrors {
            partial: document, ..
        }) => document.to_string(),
    };

    assert_eq!(
        formatted2.trim(),
        expected_query.trim(),
        "Formatted query should match baseline"
    );
}

#[tokio::test]
async fn test_format_code_action_with_baseline() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type User { id: ID name: String email: String } type Query { me: User }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.ts".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();
    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    // Use fixture content
    let ts_path = base_dir.join("query.ts");
    let ts_text = load_fixture("cramped_query.ts");
    fs::write(&ts_path, &ts_text).unwrap();
    let ts_path = std::fs::canonicalize(ts_path).unwrap();
    let ts_uri = Url::from_file_path(&ts_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: ts_uri.clone(),
                            language_id: "typescript".to_string(),
                            version: 1,
                            text: ts_text.clone(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Request code actions
    let range = Range::new(Position::new(0, 16), Position::new(0, 16));
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: ts_uri.clone(),
        },
        range,
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let request = Request::build("textDocument/codeAction")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<CodeActionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let actions = result.expect("Expected actions");
    let format_action = actions
        .iter()
        .find(|a| {
            if let CodeActionOrCommand::CodeAction(ca) = a {
                ca.title == "Format GraphQL"
            } else {
                false
            }
        })
        .expect("Should find 'Format GraphQL' action");

    if let CodeActionOrCommand::CodeAction(action) = format_action {
        let edit = action.edit.as_ref().unwrap();
        let changes = edit.changes.as_ref().unwrap();
        let edits = &changes[&ts_uri];

        assert_eq!(edits.len(), 1);
        let formatted_text = &edits[0].new_text;

        // Load expected baseline
        let expected = load_baseline("cramped_query.expected.graphql");

        assert_eq!(
            formatted_text.trim(),
            expected.trim(),
            "Format action should produce baseline-matching output"
        );
    }
}
