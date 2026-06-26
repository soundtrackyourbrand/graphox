use crate::backend::state::Backend;
use ahash::AHashSet;
use graphox_features::code_actions::{DocumentCodeActions, SOURCE_FIX_ALL_GRAPHOX};

use tower_lsp::jsonrpc::{Error, Result};
use tower_lsp::lsp_types::*;

fn action_kind_matches(kind: CodeActionKind, filters: &[CodeActionKind]) -> bool {
    let action_kind = kind.as_str();
    filters.iter().any(|filter| {
        let filter = filter.as_str();
        action_kind == filter
            || action_kind
                .strip_prefix(filter)
                .is_some_and(|suffix| suffix.is_empty() || suffix.starts_with('.'))
    })
}

pub async fn handle_code_action(
    backend: &Backend,
    params: CodeActionParams,
) -> Result<Option<CodeActionResponse>> {
    backend
        .with_tracing("code_action", async move {
            let uri = &params.text_document.uri;
            let requested_kinds = params.context.only.clone().unwrap_or_default();
            let no_filter = requested_kinds.is_empty();

            let include_quickfix =
                no_filter || action_kind_matches(CodeActionKind::QUICKFIX, &requested_kinds);
            let include_refactor = no_filter
                || action_kind_matches(CodeActionKind::REFACTOR_EXTRACT, &requested_kinds);
            // Match against the concrete kind we actually emit (`source.fixAll.graphox`).
            // Because `action_kind_matches` treats a request for a parent kind as
            // matching its sub-kinds, this is requested by `source`, `source.fixAll`,
            // AND the specific `source.fixAll.graphox` — the last of which the previous
            // `SOURCE_FIX_ALL`-only check missed, so on-save formatting was skipped.
            let include_source =
                no_filter || action_kind_matches(SOURCE_FIX_ALL_GRAPHOX, &requested_kinds);

            if !include_quickfix && !include_refactor && !include_source {
                return Ok(None);
            }

            let mut actions = Vec::new();
            let mut seen_diagnostics = AHashSet::default();
            let doc = backend.documents.get(uri).map(|r| r.value().clone());

            // 1. Diagnostics-based fixes
            if include_quickfix {
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

                            if let Some(doc) = doc.as_ref() {
                                let type_only_actions =
                                    doc.get_unused_fragment_actions(&diagnostic);
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
                            let mut target_uri = uri.clone();
                            let mut target_range = diagnostic.range;

                            if let Some(data) = &diagnostic.data {
                                if let Some(def_uri) = data.get("def_uri").and_then(|v| v.as_str())
                                    && let Ok(parsed) = Url::parse(def_uri)
                                {
                                    target_uri = parsed;
                                }
                                if let Some(def_range) = data.get("def_range")
                                    && let Ok(r) =
                                        serde_json::from_value::<Range>(def_range.clone())
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

                            if let Some(d) = &diagnostic.data {
                                ca.data = Some(d.clone());
                            }

                            actions.push(CodeActionOrCommand::CodeAction(ca));
                        } else if code == "missing_field" {
                            if let Some(doc) = doc.as_ref() {
                                let field_actions = doc.get_missing_field_actions(&diagnostic);
                                for action in field_actions {
                                    actions.push(CodeActionOrCommand::CodeAction(action));
                                }
                            }
                        } else if code == "no_duplicate_fields" {
                            if let Some(doc) = doc.as_ref() {
                                let actions_for_dup = doc.get_duplicate_field_actions(&diagnostic);
                                for action in actions_for_dup {
                                    actions.push(CodeActionOrCommand::CodeAction(action));
                                }
                            }
                        } else if code == "required_field_missing" {
                            if let Some(doc) = doc.as_ref() {
                                let field_actions = doc.get_required_field_actions(&diagnostic);
                                for action in field_actions {
                                    actions.push(CodeActionOrCommand::CodeAction(action));
                                }
                            }
                        } else if code == "forbidden_field_selected" {
                            if let Some(doc) = doc.as_ref() {
                                let field_actions = doc.get_forbidden_field_actions(&diagnostic);
                                for action in field_actions {
                                    actions.push(CodeActionOrCommand::CodeAction(action));
                                }
                            }
                        } else if code == "deprecated"
                            && let Some(doc) = doc.as_ref()
                        {
                            let field_actions = doc.get_deprecation_actions(&diagnostic);
                            for action in field_actions {
                                actions.push(CodeActionOrCommand::CodeAction(action));
                            }
                        }
                    }
                }
            }

            // 2. Refactoring actions
            if let Some(doc) = doc.as_ref() {
                if include_refactor {
                    let schema = backend.get_schema_for_doc(uri);
                    let refactor_actions = doc.get_extraction_actions(params.range, &schema);
                    for action in refactor_actions {
                        actions.push(CodeActionOrCommand::CodeAction(action));
                    }
                }

                // 3. Format action for inline GraphQL blocks
                if include_source && let Some(format_action) = doc.get_format_action(params.range) {
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
