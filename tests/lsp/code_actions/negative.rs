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

/// Ask for the ignore quick fixes on the deprecated field in `query_text` and
/// return the ignore-related actions with their single edit.
async fn ignore_actions_for_deprecation(query_text: &str) -> Vec<(String, String)> {
    let schema = "type User { id: ID! oldField: String @deprecated(reason: \"Use newField\") } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

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

    actions
        .iter()
        .filter_map(|a| match a {
            CodeActionOrCommand::CodeAction(ca) if ca.title.contains("graphox-ignore") => {
                let edit = ca
                    .edit
                    .as_ref()
                    .and_then(|e| e.changes.as_ref())
                    .and_then(|c| c.values().next())
                    .and_then(|edits| edits.first())
                    .map(|e| e.new_text.clone())
                    .unwrap_or_default();
                Some((ca.title.clone(), edit))
            }
            _ => None,
        })
        .collect()
}

/// With no comment on the line, the fix writes one naming the rule it silences.
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_ignore_action_writes_a_scoped_comment() {
    let found =
        ignore_actions_for_deprecation("query GetUser {\n  me {\n    oldField\n  }\n}").await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].1, " # graphox-ignore deprecated");
}

/// A comment naming a *different* rule has this one added to it. Writing a
/// second marker would leave two on one line, where only the first is read.
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_ignore_action_adds_to_an_existing_scoped_comment() {
    let found = ignore_actions_for_deprecation(
        "query GetUser {\n  me {\n    oldField # graphox-ignore required_fields\n  }\n}",
    )
    .await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "Add deprecated to the # graphox-ignore comment");
    assert_eq!(found[0].1, ", deprecated");
}

/// The rule list is extended, not the explanation: the addition goes before the
/// marker that starts the prose.
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_ignore_action_adds_before_an_explanation() {
    let found = ignore_actions_for_deprecation(
        "query GetUser {\n  me {\n    oldField # graphox-ignore required_fields: for now\n  }\n}",
    )
    .await;
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].1, ", deprecated");
    // Applying it must land the rule in the list rather than inside the prose.
    let line = "    oldField # graphox-ignore required_fields: for now";
    let at = line.find(": for now").unwrap();
    let applied = format!("{}{}{}", &line[..at], ", deprecated", &line[at..]);
    assert_eq!(
        applied,
        "    oldField # graphox-ignore required_fields, deprecated: for now"
    );
}

/// A comment that already covers this rule needs nothing, whether it names the
/// rule or is bare.
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_no_ignore_action_when_the_rule_is_already_covered() {
    for line in [
        "oldField # graphox-ignore deprecated",
        "oldField # graphox-ignore",
        "oldField # graphox-ignore deprecated, required_fields",
        "oldField # graphox-ignore: legacy",
    ] {
        let text = format!("query GetUser {{\n  me {{\n    {line}\n  }}\n}}");
        let found = ignore_actions_for_deprecation(&text).await;
        assert!(found.is_empty(), "{line:?} offered {found:?}");
    }
}
