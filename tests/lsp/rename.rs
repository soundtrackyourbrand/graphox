use graphql_rust::{
    Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
};
use std::fs;
use tempfile::tempdir;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test]
async fn test_fragment_rename() {
    let (tmpdir, config) = crate::support::make_temp_project_with_schema(
        "type Query { user: User } type User { id: ID! name: String }",
        "**/*.graphql",
    );
    // Keep package.json like the original test
    fs::write(tmpdir.path().join("package.json"), "{}").unwrap();

    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    // 1. Fragment file
    let fragment_text = "fragment UserFields on User { id name }";
    let fragment_uri = crate::support::write_project_file(&tmpdir, "user_fragment.graphql", fragment_text);
    crate::support::lsp_did_open(&mut service, fragment_uri.clone(), "graphql", 1, fragment_text).await;

    // 2. Query file
    let query_text = "query { user { ...UserFields } }";
    let query_uri = crate::support::write_project_file(&tmpdir, "query.graphql", query_text);
    crate::support::lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // 3. Trigger Rename on "UserFields" in fragment file to "MyFields"
    let position = Position::new(0, 9);
    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: fragment_uri.clone(),
            },
            position,
        },
        new_name: "MyFields".to_string(),
        work_done_progress_params: Default::default(),
    };

    let request = Request::build("textDocument/rename")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<WorkspaceEdit> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let edit = result.expect("Expected WorkspaceEdit");
    let changes = edit.changes.expect("Expected changes");

    assert_eq!(changes.len(), 2);

    let frag_edits = &changes[&fragment_uri];
    assert_eq!(frag_edits.len(), 1);
    assert_eq!(frag_edits[0].new_text, "MyFields");
    assert_eq!(frag_edits[0].range.start.character, 9);

    let query_edits = &changes[&query_uri];
    assert_eq!(query_edits.len(), 1);
    assert_eq!(query_edits[0].new_text, "MyFields");
    assert_eq!(query_edits[0].range.start.character, 18);
}

#[tokio::test]
async fn test_fragment_rename_tsx() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User } type User { id: ID! name: String }",
    )
    .unwrap();

    let _config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.{graphql,tsx}".to_string()),
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

    let (tmpdir, config) = crate::support::make_temp_project_with_schema(
        "type Query { user: User } type User { id: ID! name: String }",
        "**/*.{graphql,tsx}",
    );
    fs::write(tmpdir.path().join("package.json"), "{}").unwrap();

    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    // 1. Fragment file
    let fragment_text = "fragment UserFields on User { id name }";
    let fragment_uri = crate::support::write_project_file(&tmpdir, "user_fragment.graphql", fragment_text);
    crate::support::lsp_did_open(&mut service, fragment_uri.clone(), "graphql", 1, fragment_text).await;

    // 2. TSX file
    let tsx_text = r#"
        const query = gql`
            query {
                user {
                    ...UserFields
                }
            }
        `;
    "#;
    let tsx_uri = crate::support::write_project_file(&tmpdir, "Component.tsx", tsx_text);
    crate::support::lsp_did_open(&mut service, tsx_uri.clone(), "typescriptreact", 1, tsx_text).await;

    // 3. Trigger Rename on "UserFields" in fragment file
    let position = Position::new(0, 9);
    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: fragment_uri.clone(),
            },
            position,
        },
        new_name: "RenamedFields".to_string(),
        work_done_progress_params: Default::default(),
    };

    let request = Request::build("textDocument/rename")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<WorkspaceEdit> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let edit = result.expect("Expected WorkspaceEdit");
    let changes = edit.changes.expect("Expected changes");

    assert_eq!(changes.len(), 2);

    let tsx_edits = &changes[&tsx_uri];
    assert_eq!(tsx_edits.len(), 1);
    assert_eq!(tsx_edits[0].new_text, "RenamedFields");

    // Verify it correctly identified the location in TSX
    let line = tsx_text
        .lines()
        .nth(tsx_edits[0].range.start.line as usize)
        .unwrap();
    assert!(line.contains("...UserFields"));
}

#[tokio::test]
async fn test_rename_unopened_file() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user: User } type User { id: ID! name: String }",
    )
    .unwrap();

    // 1. Fragment file (will be opened)
    let fragment_text = "fragment UserFields on User { id name }";
    // We'll create the fragment inside the test workspace (tmpdir) and open it.

    let _config = Config {
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
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        lsp_codegen_throttle_ms: None,
        codegen_watch_debounce_ms: None,
        ..Config::new_empty()
    };

    let (tmpdir, config) = crate::support::make_temp_project_with_schema(
        "type Query { user: User } type User { id: ID! name: String }",
        "**/*.graphql",
    );
    fs::write(tmpdir.path().join("package.json"), "{}").unwrap();

    // Write the query file into the workspace BEFORE initializing the LSP service so the
    // workspace scan discovers it. The test expects an unopened file to be included in
    // the rename `WorkspaceEdit`.
    let query_text = "query { user { ...UserFields } }";
    let query_uri = crate::support::write_project_file(&tmpdir, "query.graphql", query_text);

    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    // Open only the fragment file
    let fragment_uri = crate::support::write_project_file(&tmpdir, "user_fragment.graphql", fragment_text);
    crate::support::lsp_did_open(&mut service, fragment_uri.clone(), "graphql", 1, fragment_text).await;

    // Trigger Rename on "UserFields" in fragment file to "MyFields"
    let position = Position::new(0, 9);
    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: fragment_uri.clone(),
            },
            position,
        },
        new_name: "MyFields".to_string(),
        work_done_progress_params: Default::default(),
    };

    let request = Request::build("textDocument/rename")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<WorkspaceEdit> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let edit = result.expect("Expected WorkspaceEdit");
    let changes = edit.changes.expect("Expected changes");

    // Check if both files are present in the changes
    assert!(
        changes.contains_key(&fragment_uri),
        "Changes should include fragment file"
    );
    assert!(
        changes.contains_key(&query_uri),
        "Changes should include unopened query file"
    );

    let frag_edits = &changes[&fragment_uri];
    assert_eq!(frag_edits.len(), 1);
    assert_eq!(frag_edits[0].new_text, "MyFields");

    let query_edits = &changes[&query_uri];
    assert_eq!(query_edits.len(), 1);
    assert_eq!(query_edits[0].new_text, "MyFields");
    assert_eq!(query_edits[0].range.start.character, 18);
}
