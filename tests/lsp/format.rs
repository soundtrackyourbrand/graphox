use crate::support::create_doc;
use std::fs;
use std::path::PathBuf;
use tower_lsp::lsp_types::*;

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
    let doc = create_doc(uri_str, file_content);

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
#[ntest::timeout(3000)]
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
#[ntest::timeout(3000)]
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
#[ntest::timeout(3000)]
fn test_format_fragment_spread() {
    let fixture = load_fixture("fragment_spread.ts");
    let expected_fragment = load_baseline("fragment_spread_fragment.expected.graphql");
    let expected_query = load_baseline("fragment_spread_query.expected.graphql");

    let doc = create_doc("file:///test.ts", &fixture);

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
#[ntest::timeout(3000)]
async fn test_format_code_action_with_baseline() {
    let (dir, config) = crate::support::make_temp_project_with_schema(
        "type User { id: ID name: String email: String } type Query { me: User }",
        "**/*.ts",
    );

    // Use fixture content
    let ts_text = load_fixture("cramped_query.ts");

    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    let ts_uri = crate::support::write_project_file(&dir, "query.ts", &ts_text);
    crate::support::lsp_did_open(&mut service, ts_uri.clone(), "typescript", 1, &ts_text).await;

    let doc = create_doc(ts_uri.as_str(), &ts_text);

    // Request code actions
    let range = crate::support::range_for_token(&doc, &ts_text, "query");
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

    let result = crate::support::lsp_request_code_actions(&mut service, params, 1).await;

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

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_format_code_action_preserves_comments_and_template_indentation() {
    let (dir, config) = crate::support::make_temp_project_with_schema(
        "type Query { radioPlaylist(id: ID!, kind: RadioPlaylistKind): RadioPlaylist } enum RadioPlaylistKind { A } type RadioPlaylist { id: ID playlist: Playlist } type Playlist { id: ID permissions: String name: String composerType: String }",
        "**/*.tsx",
    );

    let tsx_text = r#"export const SourceRadioDoc = graphql(/* GraphQL */ `
  query SourceRadio($id: ID!, $kind: RadioPlaylistKind) {
    # eslint-disable-next-line @graphql-eslint/require-id-when-available
    radioPlaylist(id: $id, kind: $kind) { # graphox-ignore: fetching permissions on RadioPlaylistComposer is broken
      id,
      playlist {
        permissions,
        name,
        composerType,
        id,
        ...Displayable,
      }
    }
  }
`)
"#;

    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    let tsx_uri = crate::support::write_project_file(&dir, "query.tsx", tsx_text);
    crate::support::lsp_did_open(
        &mut service,
        tsx_uri.clone(),
        "typescriptreact",
        1,
        tsx_text,
    )
    .await;

    let doc = create_doc(tsx_uri.as_str(), tsx_text);
    let range = crate::support::range_for_token(&doc, tsx_text, "SourceRadio");
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: tsx_uri.clone(),
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

    let result = crate::support::lsp_request_code_actions(&mut service, params, 1).await;
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
        let edits = &changes[&tsx_uri];

        assert_eq!(edits.len(), 1);
        let formatted_text = &edits[0].new_text;
        let expected = r#"
  query SourceRadio($id: ID!, $kind: RadioPlaylistKind) {
    # eslint-disable-next-line @graphql-eslint/require-id-when-available
    radioPlaylist(id: $id, kind: $kind) { # graphox-ignore: fetching permissions on RadioPlaylistComposer is broken
      id
      playlist {
        permissions
        name
        composerType
        id
        ...Displayable
      }
    }
  }
"#;

        assert_eq!(
            formatted_text, expected,
            "Format action should preserve GraphQL comments and embedded indentation"
        );
    }
}
