use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    pos, write_project_file,
};
use std::fs;
use tempfile::tempdir;
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_fragment_references() {
    let (dir, config) = make_temp_project_with_schema(
        "type Query { user: User } type User { id: ID! name: String }",
        "**/*.graphql",
    );
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Create and Open the fragment definition file
    let fragment_text = "fragment UserFields on User { id name }";
    let fragment_uri = write_project_file(&dir, "user_fragment.graphql", fragment_text);
    lsp_did_open(&mut service, fragment_uri.clone(), "graphql", 1, fragment_text).await;

    // 2. Create and Open the query file that uses the fragment
    let query_text = "query GetUser { user { ...UserFields } }";
    let query_uri = write_project_file(&dir, "query_with_fragment.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // 3. Trigger Find References on "UserFields" in fragment file
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: fragment_uri.clone(),
            },
            position: pos(0, 9),
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
    assert_eq!(locations.len(), 2); // 1 definition + 1 spread

    let has_def = locations
        .iter()
        .any(|l| l.uri == fragment_uri && l.range.start.character == 9);
    let has_spread = locations
        .iter()
        .any(|l| l.uri == query_uri && l.range.start.character == 26);

    assert!(has_def, "Missing definition in references");
    assert!(has_spread, "Missing spread in references");
}

#[tokio::test]
async fn test_fragment_references_tsx() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();

    let schema_text = "type Query { user: User } type User { id: ID! name: String }";
    let (tmpdir, config) = make_temp_project_with_schema(schema_text, "**/*.{graphql,tsx}");

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Fragment in .graphql file
    let fragment_text = "fragment UserFields on User { id name }";
    let fragment_uri = write_project_file(&tmpdir, "user_fragment.graphql", fragment_text);
    lsp_did_open(&mut service, fragment_uri.clone(), "graphql", 1, fragment_text).await;

    // 2. Usage in .tsx file
    let tsx_text = r#"
        import { gql } from './gql';
        const query = gql`
            query GetUser {
                user {
                    ...UserFields
                }
            }
        `;
    "#;
    let tsx_uri = write_project_file(&tmpdir, "Component.tsx", tsx_text);
    lsp_did_open(&mut service, tsx_uri.clone(), "typescriptreact", 1, tsx_text).await;

    // 3. Trigger Find References on "UserFields" in fragment file
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: fragment_uri.clone(),
            },
            position: pos(0, 9),
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
    assert_eq!(locations.len(), 2);

    let has_tsx_spread = locations.iter().any(|l| {
        l.uri == tsx_uri
            && tsx_text
                .lines()
                .nth(l.range.start.line as usize)
                .unwrap()
                .contains("...UserFields")
    });
    assert!(has_tsx_spread, "Missing TSX spread in references");
}

#[tokio::test]
async fn test_fragment_references_exclude_declaration() {
    let schema_text = "type Query { user: User } type User { id: ID! name: String }";
    let (dir, config) = make_temp_project_with_schema(schema_text, "**/*.graphql");

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Fragment file
    let fragment_text = "fragment UserFields on User { id name }";
    let fragment_uri = write_project_file(&dir, "user_fragment.graphql", fragment_text);
    lsp_did_open(&mut service, fragment_uri.clone(), "graphql", 1, fragment_text).await;

    // 2. Query file
    let query_text = "query { user { ...UserFields } }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // 3. Trigger Find References with include_declaration: false
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: fragment_uri.clone(),
            },
            position: pos(0, 9),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: false,
        },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    let locations = result.expect("Expected locations");
    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].uri, query_uri);
}