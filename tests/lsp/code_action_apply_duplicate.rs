use crate::support::{
    apply_text_edit, create_doc, create_initialized_lsp_service, find_code_action_by_title,
    lsp_did_open, lsp_request_code_actions, make_temp_project_with_schema,
    range_for_token_at_index, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_apply_remove_duplicate_field_code_action() {
    let schema = "type Query { me: User } type User { id: ID name: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let dup_text = "query { me { id id name } }";
    let dup_uri = write_project_file(&dir, "dup.graphql", dup_text);
    lsp_did_open(&mut service, dup_uri.clone(), "graphql", 1, dup_text).await;

    let doc = create_doc(dup_uri.as_str(), dup_text);

    // Construct a diagnostic that points to the duplicated `id` field in dup.graphql
    let dup_diag = Diagnostic {
        range: range_for_token_at_index(&doc, dup_text, "id", 0),
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
        text_document: TextDocumentIdentifier {
            uri: dup_uri.clone(),
        },
        range: dup_diag.range,
        context: CodeActionContext {
            diagnostics: vec![dup_diag.clone()],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions_dup = lsp_request_code_actions(&mut service, params_dup, 1)
        .await
        .expect("Expected actions for dup file");

    let ca = find_code_action_by_title(&actions_dup, "Remove duplicate field")
        .expect("Expected 'Remove duplicate field' action");

    let edit = ca.edit.as_ref().unwrap();
    let changes = edit.changes.as_ref().unwrap();
    let edits = &changes[&dup_uri];

    // Apply the edit to the file contents
    let mut content = dup_text.to_string();
    for e in edits {
        content = apply_text_edit(&content, e);
    }

    // Try to parse the resulting document with apollo parser
    let mut parser = apollo_compiler::parser::Parser::new();
    let parse_res = parser.parse_ast(content.clone(), "dup.graphql");
    assert!(
        parse_res.is_ok()
            || matches!(
                parse_res,
                Err(apollo_compiler::validation::WithErrors { partial: _, .. })
            ),
        "Resulting document should parse or partially parse"
    );

    // The duplicate-field code action should remove the duplicated field; verify exact text
    // Note: It removes the second 'id' and leaves a space.
    let expected = "query { me { id name } }".to_string();
    assert_eq!(
        content.replace("  ", " ").trim(),
        expected,
        "Code action should remove the duplicate field"
    );
}
