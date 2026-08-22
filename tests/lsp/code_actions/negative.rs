use crate::support::{
    create_doc, create_initialized_lsp_service, lsp_did_open, lsp_request_code_actions,
    make_temp_project_with_schema, write_project_file,
};
use tower_lsp_server::ls_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_no_actions_without_diagnostic() {
    let schema = "type Query { me: String }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let query_text = "query { me }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let doc = create_doc(query_uri.as_str(), query_text);
    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range: Range {
            start: doc.byte_to_position(0),
            end: doc.byte_to_position(5),
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions_opt = lsp_request_code_actions(&mut service, params, 1).await;
    let actions = match actions_opt {
        Some(a) => a,
        None => {
            return;
        }
    };

    let has_quickfix = actions.iter().any(|a| {
        if let CodeActionOrCommand::CodeAction(ca) = a {
            ca.kind == Some(CodeActionKind::QUICKFIX)
        } else {
            false
        }
    });
    assert!(
        !has_quickfix,
        "Should not return quickfix actions without diagnostic"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_already_ignored_field() {
    let schema = "type User { id: ID! oldField: String @deprecated(reason: \"Use newField\") } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let query_text = "query GetUser {\n  me {\n    oldField # graphox-ignore\n  }\n}";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let doc = create_doc(query_uri.as_str(), query_text);
    let diagnostic = Diagnostic {
        range: crate::support::range_for_token(&doc, query_text, "oldField"),
        message: "Field 'oldField' is deprecated: Use newField".to_string(),
        code: Some(NumberOrString::String("deprecated".to_string())),
        severity: Some(DiagnosticSeverity::WARNING),
        ..Default::default()
    };

    let params = CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: query_uri.clone(),
        },
        range: diagnostic.range,
        context: CodeActionContext {
            diagnostics: vec![diagnostic],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let actions = lsp_request_code_actions(&mut service, params, 1)
        .await
        .expect("Expected actions");

    let ignore_action = actions.iter().find(|a| {
        if let CodeActionOrCommand::CodeAction(ca) = a {
            ca.title.contains("graphox-ignore")
        } else {
            false
        }
    });

    assert!(
        ignore_action.is_none(),
        "Should NOT return ignore action when field already has graphox-ignore"
    );
}
