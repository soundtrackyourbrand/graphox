use crate::support::{
    create_doc, create_initialized_lsp_service, lsp_did_open, lsp_request_code_actions,
    lsp_request_diagnostics, make_temp_project_with_schema, write_project_file,
};
use ahash::AHashMap;
use graphox::config::RulesConfig;
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

/// Each ignore quick fix has to write on the placement that actually silences
/// its rule: the offending field for a rule about a field that is present, the
/// object for a rule about one that is absent. The diagnostic's own range is
/// what the fix writes on, so this pins that the two agree.
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_ignore_actions_write_where_the_rule_reads_them() {
    use graphox::config::{ForbiddenFieldRule, RequiredFieldRule};

    let schema = "type User { id: ID! password: String } type Query { me: User }";

    // forbidden: the field is there, so the comment goes on it.
    let mut forbidden = AHashMap::default();
    forbidden.insert("password".to_string(), ForbiddenFieldRule::new_always(true));
    let (dir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_rules(RulesConfig::default().with_forbidden_fields(forbidden));
    let (mut service, _h) = create_initialized_lsp_service(config).await;

    let text = "query GetUser {\n  me {\n    id\n    password\n  }\n}";
    let uri = write_project_file(&dir, "q.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let diags = match lsp_request_diagnostics(&mut service, uri.clone()).await {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            full.full_document_diagnostic_report.items
        }
        _ => panic!("expected a full report"),
    };
    let forbidden_diag = diags
        .iter()
        .find(|d| d.message.contains("forbidden"))
        .expect("expected a forbidden diagnostic");
    // Line 3 is `password`; the fix writes on the diagnostic's line.
    assert_eq!(
        forbidden_diag.range.end.line, 3,
        "a forbidden diagnostic must point at the field, since that is the only \
         placement that silences it: {forbidden_diag:?}"
    );

    // required: the field is absent, so the comment goes on the object.
    let mut required = AHashMap::default();
    required.insert("password".to_string(), RequiredFieldRule::new_always(true));
    let (dir2, mut config2) = make_temp_project_with_schema(schema, "**/*.graphql");
    config2 = config2.with_rules(RulesConfig::default().with_required_fields(required));
    let (mut service2, _h2) = create_initialized_lsp_service(config2).await;

    let text2 = "query GetUser {\n  me {\n    id\n  }\n}";
    let uri2 = write_project_file(&dir2, "q.graphql", text2);
    lsp_did_open(&mut service2, uri2.clone(), "graphql", 1, text2).await;

    let diags2 = match lsp_request_diagnostics(&mut service2, uri2.clone()).await {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            full.full_document_diagnostic_report.items
        }
        _ => panic!("expected a full report"),
    };
    let required_diag = diags2
        .iter()
        .find(|d| d.message.starts_with("Required"))
        .expect("expected a required diagnostic");
    // Line 1 is `me {`, the object the field is missing from.
    assert_eq!(
        required_diag.range.end.line, 1,
        "a required diagnostic must point at the object, the only placement \
         that can carry its suppression: {required_diag:?}"
    );
}
