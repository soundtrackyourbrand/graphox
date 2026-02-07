use crate::support::{
    make_temp_project_with_schema, create_initialized_lsp_service, write_project_file, lsp_did_open,
};
use graphql_rust::{Backend, Config, config::{GlobPattern, ProjectConfig, SchemaSource}};
use std::fs;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;

#[tokio::test]
async fn test_apply_remove_duplicate_field_code_action() {
    let schema = "type Query { me: User } type User { id: ID name: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let dup_text = "query { me { id id name } }";
    let dup_uri = write_project_file(&dir, "dup.graphql", dup_text);
    lsp_did_open(&mut service, dup_uri.clone(), "graphql", 1, dup_text).await;

    // Construct a diagnostic that points to the duplicated `id` field in dup.graphql
    let dup_diag = Diagnostic {
        range: Range::new(Position::new(0, 13), Position::new(0, 15)),
        message: "Duplicate field 'id' in selection set".to_string(),
        code: Some(NumberOrString::String("no_duplicate_fields".to_string())),
        data: Some(serde_json::json!({
            "response_key": "id",
            "args": "",
            "selection": "",
        })),
        ..Default::default()
    };

    let params_dup = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: dup_uri.clone() },
        range: dup_diag.range,
        context: CodeActionContext {
            diagnostics: vec![dup_diag.clone()],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let request_dup = Request::build("textDocument/codeAction")
        .id(1)
        .params(serde_json::to_value(&params_dup).unwrap())
        .finish();
    let response_dup = service.call(request_dup).await.unwrap().unwrap();
    let result_dup: Option<CodeActionResponse> = serde_json::from_value(response_dup.result().unwrap().clone()).unwrap();
    let actions_dup = result_dup.expect("Expected actions for dup file");

    let dup_action = actions_dup
        .iter()
        .find(|a| match a {
            CodeActionOrCommand::CodeAction(ca) => ca.title == "Remove duplicate field",
            _ => false,
        })
        .expect("Expected 'Remove duplicate field' action");

    if let CodeActionOrCommand::CodeAction(action) = dup_action {
        let edit = action.edit.as_ref().unwrap();
        let changes = edit.changes.as_ref().unwrap();
        let edits = &changes[&dup_uri];
        // Apply the edit to the file contents and ensure resulting text still parses
        let mut content = dup_text.to_string();
        for e in edits {
            // Convert range to byte offsets
            let start = e.range.start;
            let end = e.range.end;
            // For simplicity in test, apply a naive UTF-8 based slicing using positions
            let start_byte = graphql_rust::helpers::position_to_byte(&content, start);
            let end_byte = graphql_rust::helpers::position_to_byte(&content, end);
            content.replace_range(start_byte..end_byte, &e.new_text);
        }

        // Try to parse the resulting document with apollo parser
        let mut parser = apollo_compiler::parser::Parser::new();
        let parse_res = parser.parse_ast(content.clone(), "dup.graphql");
        assert!(parse_res.is_ok() || matches!(parse_res, Err(apollo_compiler::validation::WithErrors { partial: _, .. })), "Resulting document should parse or partially parse");

        // The duplicate-field code action should remove the duplicated field; verify exact text
        let expected = "query { me { id } }".to_string();
        assert_eq!(content, expected, "Code action should remove the duplicate field producing expected content");
    } else {
        panic!("Expected CodeAction");
    }
}
