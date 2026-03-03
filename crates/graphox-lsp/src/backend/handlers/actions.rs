use crate::backend::state::Backend;
use ahash::AHashSet;
use graphox_features::code_actions::DocumentCodeActions;

use tower_lsp::jsonrpc::{Error, Result};
use tower_lsp::lsp_types::*;

pub async fn handle_code_action(
    backend: &Backend,
    params: CodeActionParams,
) -> Result<Option<CodeActionResponse>> {
    backend
        .with_tracing("code_action", async move {
            let uri = &params.text_document.uri;
            let mut actions = Vec::new();
            let mut seen_diagnostics = AHashSet::default();

            // 1. Diagnostics-based fixes
            for diagnostic in params.context.diagnostics {
                let diagnostic_code = match &diagnostic.code {
                    Some(NumberOrString::String(s)) => s.clone(),
                    Some(NumberOrString::Number(n)) => n.to_string(),
                    None => String::new(),
                };
                let diagnostic_data = diagnostic
                    .data
                    .as_ref()
                    .and_then(|d| serde_json::to_string(d).ok())
                    .unwrap_or_default();
                let diagnostic_key = format!(
                    "{}:{}:{}:{}:{}:{}:{}:{}",
                    diagnostic.range.start.line,
                    diagnostic.range.start.character,
                    diagnostic.range.end.line,
                    diagnostic.range.end.character,
                    diagnostic_code,
                    diagnostic.message,
                    diagnostic.source.clone().unwrap_or_default(),
                    diagnostic_data
                );
                if !seen_diagnostics.insert(diagnostic_key) {
                    continue;
                }

                if let Some(NumberOrString::String(ref code)) = diagnostic.code {
                    if code == "unused_fragment" {
                        let mut changes = std::collections::HashMap::new();
                        changes.insert(
                            uri.clone(),
                            vec![TextEdit {
                                range: diagnostic.range,
                                new_text: String::new(),
                            }],
                        );

                        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                            title: "Remove unused fragment".to_string(),
                            kind: Some(CodeActionKind::QUICKFIX),
                            edit: Some(WorkspaceEdit {
                                changes: Some(changes),
                                ..Default::default()
                            }),
                            diagnostics: Some(vec![diagnostic.clone()]),
                            is_preferred: Some(true),
                            ..Default::default()
                        }));

                        if let Some(doc) = backend.documents.get(uri).map(|r| r.value().clone()) {
                            let type_only_actions = doc.get_unused_fragment_actions(&diagnostic);
                            for action in type_only_actions {
                                actions.push(CodeActionOrCommand::CodeAction(action));
                            }
                        }
                    } else if code == "unused_variable" {
                        let mut changes = std::collections::HashMap::new();
                        changes.insert(
                            uri.clone(),
                            vec![TextEdit {
                                range: diagnostic.range,
                                new_text: String::new(),
                            }],
                        );

                        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                            title: "Remove unused variable".to_string(),
                            kind: Some(CodeActionKind::QUICKFIX),
                            edit: Some(WorkspaceEdit {
                                changes: Some(changes),
                                ..Default::default()
                            }),
                            diagnostics: Some(vec![diagnostic.clone()]),
                            is_preferred: Some(true),
                            ..Default::default()
                        }));
                    } else if code == "type_only_used" {
                        // The diagnostic may originate from a fragment spread in another file.
                        // We support a quickfix that removes the @type_only directive from the
                        // fragment definition. The diagnostic.data may include the definition
                        // uri and optional def_range for the directive location.
                        let mut target_uri = uri.clone();
                        let mut target_range = diagnostic.range;

                        if let Some(data) = &diagnostic.data {
                            if let Some(def_uri) = data.get("def_uri").and_then(|v| v.as_str())
                                && let Ok(parsed) = Url::parse(def_uri)
                            {
                                target_uri = parsed;
                            }
                            if let Some(def_range) = data.get("def_range")
                                && let Ok(r) = serde_json::from_value::<Range>(def_range.clone())
                            {
                                target_range = r;
                            }
                        }

                        let mut changes = std::collections::HashMap::new();
                        changes.insert(
                            target_uri.clone(),
                            vec![TextEdit {
                                range: target_range,
                                new_text: String::new(),
                            }],
                        );

                        let mut ca = CodeAction {
                            title: "Remove @type_only directive".to_string(),
                            kind: Some(CodeActionKind::QUICKFIX),
                            edit: Some(WorkspaceEdit {
                                changes: Some(changes),
                                ..Default::default()
                            }),
                            diagnostics: Some(vec![diagnostic.clone()]),
                            is_preferred: Some(true),
                            ..Default::default()
                        };

                        // Preserve diagnostic.data so clients can inspect where the definition lives
                        if let Some(d) = &diagnostic.data {
                            ca.data = Some(d.clone());
                        }

                        actions.push(CodeActionOrCommand::CodeAction(ca));
                    } else if code == "missing_field" {
                        if let Some(doc) = backend.documents.get(uri).map(|r| r.value().clone()) {
                            let field_actions = doc.get_missing_field_actions(&diagnostic);
                            for action in field_actions {
                                actions.push(CodeActionOrCommand::CodeAction(action));
                            }
                        }
                    } else if code == "no_duplicate_fields" {
                        if let Some(doc) = backend.documents.get(uri).map(|r| r.value().clone()) {
                            let actions_for_dup = doc.get_duplicate_field_actions(&diagnostic);
                            for action in actions_for_dup {
                                actions.push(CodeActionOrCommand::CodeAction(action));
                            }
                        }
                    } else if code == "required_field_missing"
                        && let Some(doc) = backend.documents.get(uri).map(|r| r.value().clone())
                    {
                        let field_actions = doc.get_required_field_actions(&diagnostic);
                        for action in field_actions {
                            actions.push(CodeActionOrCommand::CodeAction(action));
                        }
                    } else if code == "deprecated"
                        && let Some(doc) = backend.documents.get(uri).map(|r| r.value().clone())
                    {
                        let field_actions = doc.get_deprecation_actions(&diagnostic);
                        for action in field_actions {
                            actions.push(CodeActionOrCommand::CodeAction(action));
                        }
                    }
                }
            }

            // 2. Refactoring actions
            if let Some(doc) = backend.documents.get(uri).map(|r| r.value().clone()) {
                let schema = backend.get_schema_for_doc(uri);
                let refactor_actions = doc.get_extraction_actions(params.range, &schema);
                for action in refactor_actions {
                    actions.push(CodeActionOrCommand::CodeAction(action));
                }

                // 3. Format action for inline GraphQL blocks
                if let Some(format_action) = doc.get_format_action(params.range) {
                    actions.push(CodeActionOrCommand::CodeAction(format_action));
                }
            }

            if actions.is_empty() {
                Ok(None)
            } else {
                Ok(Some(actions))
            }
        })
        .await
}

pub async fn handle_execute_command(
    backend: &Backend,
    params: ExecuteCommandParams,
) -> Result<Option<serde_json::Value>> {
    if params.command == "graphox.runCodegen" {
        backend.run_codegen().await;
        return Ok(None);
    }
    if params.command == "graphox.clearCache" {
        backend.clear_cache().await;
        return Ok(None);
    }

    Err(Error::invalid_params(format!(
        "unsupported command: {}",
        params.command
    )))
}
