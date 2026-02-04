use graphql_rust::{
    Backend, Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
};
use std::fs;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test]
async fn test_variable_operation_scoping() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { user(id: ID!): User } type User { id: ID! name: String }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("query.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
        }],
        base_dir: base_dir.to_path_buf(),
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    // Initialize
    service.call(Request::build("initialize").params(serde_json::to_value(InitializeParams::default()).unwrap()).id(0).finish()).await.unwrap().unwrap();
    service.call(Request::build("initialized").params(serde_json::json!({})).finish()).await.unwrap();

    // 1. Create a query file with TWO operations using the same variable name ($id)
    let query_path = base_dir.join("query.graphql");
    let query_text = r#"
        query Op1($id: ID!) { 
            user(id: $id) { name } 
        }

        query Op2($id: ID!) { 
            user(id: $id) { name } 
        }
    "#;
    fs::write(&query_path, query_text).unwrap();
    let query_path = std::fs::canonicalize(&query_path).unwrap();
    let query_uri = Url::from_file_path(&query_path).unwrap();

    service.call(Request::build("textDocument/didOpen").params(serde_json::to_value(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: query_uri.clone(),
            language_id: "graphql".to_string(),
            version: 1,
            text: query_text.to_string(),
        },
    }).unwrap()).finish()).await.unwrap();

    // 2. Trigger Go to Definition on "$id" inside Op2's selection set
    // query Op2($id: ID!) { user(id: $id) { name } }
    // "$id" in user(id: $id) is around Line 6, Char 30 in the r#"" string above
    // Line 6 (0-indexed) starts with 8 spaces + "user(id: $id)"
    let position = Position::new(6, 22); 
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let request = Request::build("textDocument/definition")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();

    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<GotoDefinitionResponse> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    if let Some(GotoDefinitionResponse::Scalar(location)) = result {
        assert_eq!(location.uri, query_uri);
        // Should go to Op2's definition (Line 5, character 14: "query Op2($id: ...")
        assert_eq!(location.range.start.line, 5, "Definition should be in Op2, not Op1");
        assert_eq!(location.range.start.character, 18, "Should point to the start of $id in Op2 header");
    } else {
        panic!("Expected Scalar location for variable definition, got {:?}", result);
    }

    // 3. Trigger Find References on "$id" in Op2
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri.clone() },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext { include_declaration: true },
    };

    let request = Request::build("textDocument/references").id(2).params(serde_json::to_value(&params).unwrap()).finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<Vec<Location>> = serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let locations = result.expect("Expected locations");
    // Should ONLY find Op2's definition (line 5) and usage (line 6)
    assert_eq!(locations.len(), 2, "Should only find references within the same operation");
    assert!(locations.iter().all(|l| l.range.start.line >= 5), "Should not find references in Op1");

    // 4. Trigger Rename on "$id" in Op2
    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_uri.clone() },
            position,
        },
        new_name: "$op2Id".to_string(),
        work_done_progress_params: Default::default(),
    };

    let request = Request::build("textDocument/rename").id(3).params(serde_json::to_value(&params).unwrap()).finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<WorkspaceEdit> = serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let edit = result.expect("Expected WorkspaceEdit");
    let changes = edit.changes.as_ref().expect("Expected changes");
    let query_changes = &changes[&query_uri];

    assert_eq!(query_changes.len(), 2, "Should only rename instances in Op2");
    assert!(query_changes.iter().all(|e| e.range.start.line >= 5), "Should not rename instances in Op1");
}
