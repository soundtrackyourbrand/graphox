use crate::backend::state::Backend;
use graphox_core::document::DocumentState;
use graphox_features::completion::{
    DocumentCompletion, FragmentCompletionInfo, FragmentRequirements, FragmentRequirementsResolver,
};

use ahash::AHashMap;
use std::sync::{Arc, Mutex};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::{
    CompletionItem, CompletionParams, CompletionResponse, CompletionTextEdit, Position,
    PositionEncodingKind, Range,
};

fn line_max_character(doc: &DocumentState, line: u32) -> u32 {
    let last_line = doc.rope.len_lines().saturating_sub(1) as u32;
    let line_idx = line.min(last_line) as usize;
    let mut line_text = doc.rope.line(line_idx).to_string();
    while line_text.ends_with('\n') || line_text.ends_with('\r') {
        line_text.pop();
    }

    if doc.position_encoding == PositionEncodingKind::UTF8 {
        line_text.len() as u32
    } else if doc.position_encoding == PositionEncodingKind::UTF16 {
        line_text.encode_utf16().count() as u32
    } else {
        line_text.chars().count() as u32
    }
}

fn clamp_position(doc: &DocumentState, pos: Position) -> Position {
    let max_line = doc.rope.len_lines().saturating_sub(1) as u32;
    let line = pos.line.min(max_line);
    let max_char = line_max_character(doc, line);
    Position::new(line, pos.character.min(max_char))
}

fn clamp_range(doc: &DocumentState, range: Range) -> Range {
    let start = clamp_position(doc, range.start);
    let mut end = clamp_position(doc, range.end);
    if end.line < start.line || (end.line == start.line && end.character < start.character) {
        end = start;
    }
    Range::new(start, end)
}

fn sanitize_completion_items(doc: &DocumentState, items: &mut [CompletionItem]) {
    for item in items {
        if let Some(text_edit) = &mut item.text_edit {
            match text_edit {
                CompletionTextEdit::Edit(edit) => {
                    edit.range = clamp_range(doc, edit.range);
                }
                CompletionTextEdit::InsertAndReplace(edit) => {
                    edit.insert = clamp_range(doc, edit.insert);
                    edit.replace = clamp_range(doc, edit.replace);
                }
            }
        }

        if let Some(additional_edits) = &mut item.additional_text_edits {
            for edit in additional_edits {
                edit.range = clamp_range(doc, edit.range);
            }
        }
    }
}

pub async fn handle_completion(
    backend: &Backend,
    params: CompletionParams,
) -> Result<Option<CompletionResponse>> {
    backend
        .with_tracing("completion", async move {
            let uri = backend.normalize_uri(params.text_document_position.text_document.uri);
            let position = params.text_document_position.position;

            if let Some(doc) = backend.documents.get(&uri).map(|r| r.value().clone()) {
                let schema = backend.get_schema_for_doc(&uri);

                let project_subgraphs = if let Ok(path) = uri.to_file_path()
                    && let Ok(config) = backend.config.read()
                {
                    let schema_key = config.get_schema_for_path(&path);
                    schema_key
                        .and_then(|key| backend.subgraphs.get(&key).map(|r| r.value().clone()))
                } else {
                    None
                };

                let all_fragments = backend.get_all_fragments_info();

                let fragments = backend.get_fragments_for_doc(&doc, &all_fragments);

                log::trace!(
                    "completion: fragments for doc {} = {:?}",
                    doc.uri,
                    fragments.iter().map(|f| f.name.clone()).collect::<Vec<_>>()
                );

                // Optimization: Pre-index fragments by name for faster recursive lookups
                let mut fragments_by_name: AHashMap<Arc<str>, Vec<FragmentCompletionInfo>> =
                    AHashMap::with_capacity(all_fragments.len());
                for f in all_fragments.iter() {
                    fragments_by_name
                        .entry(f.name.clone())
                        .or_default()
                        .push(f.clone());
                }

                let variable_types_cache: Mutex<AHashMap<Arc<str>, FragmentRequirements>> =
                    Mutex::new(AHashMap::default());
                let package_root = doc.package_root.clone();
                let documents = backend.documents.clone();
                let schema_for_requirements = schema.clone();
                let position_encoding = backend.get_position_encoding();

                let resolve_requirements: FragmentRequirementsResolver =
                    Arc::new(move |name: &str| {
                        let mut requirements = std::collections::BTreeMap::new();
                        let mut visited = ahash::AHashSet::<Arc<str>>::default();
                        let mut stack: Vec<Arc<str>> = vec![Arc::from(name)];

                        while let Some(current_name) = stack.pop() {
                            if !visited.insert(current_name.clone()) {
                                continue;
                            }

                            if let Some(potentials) = fragments_by_name.get(&current_name)
                                && let Some(frag) = potentials.iter().find(|p| {
                                    p.is_public
                                        || graphox_core::utils::paths_match(
                                            p.package_root.as_deref(),
                                            package_root.as_deref(),
                                        )
                                })
                            {
                                let cached_vars = {
                                    variable_types_cache
                                        .lock()
                                        .ok()
                                        .and_then(|c| c.get(&current_name).cloned())
                                };
                                let local_vars = if let Some(cached) = cached_vars {
                                    cached
                                } else {
                                    let doc_arc = if let Some(frag_doc) =
                                        documents.get(&frag.uri).map(|r| r.value().clone())
                                    {
                                        Some(frag_doc)
                                    } else if let Ok(path) = frag.uri.to_file_path()
                                        && let Ok(content) = std::fs::read_to_string(&path)
                                    {
                                        Some(Arc::new(DocumentState::new_from_thread_local(
                                            frag.uri.clone(),
                                            &content,
                                            position_encoding.clone(),
                                        )))
                                    } else {
                                        None
                                    };

                                    if let Some(frag_doc) = doc_arc {
                                        let vars = frag_doc.get_fragment_variable_types(
                                            &current_name,
                                            &schema_for_requirements,
                                        );
                                        let mut vars_arc = std::collections::BTreeMap::new();
                                        for (k, v) in vars {
                                            vars_arc.insert(Arc::from(k), Arc::from(v));
                                        }
                                        if let Ok(mut cache) = variable_types_cache.lock() {
                                            cache.insert(current_name.clone(), vars_arc.clone());
                                        }
                                        vars_arc
                                    } else {
                                        std::collections::BTreeMap::new()
                                    }
                                };

                                for (var, ty) in local_vars {
                                    requirements.insert(var, ty);
                                }

                                for nested in frag.used_fragments.iter() {
                                    stack.push(nested.clone());
                                }
                            }
                        }
                        requirements
                    });

                let doc_for_blocking = doc.clone();
                let result = tokio::task::spawn_blocking(move || {
                    doc_for_blocking.get_completion_items(
                        position,
                        &schema,
                        project_subgraphs.as_deref(),
                        &fragments,
                        resolve_requirements,
                    )
                })
                .await;

                let mut items = match result {
                    Ok(i) => i,
                    Err(_) => return Ok(None),
                };

                sanitize_completion_items(&doc, &mut items);
                log::trace!(
                    "completion: produced items = {:?}",
                    items.iter().map(|i| i.label.clone()).collect::<Vec<_>>()
                );
                return Ok(Some(CompletionResponse::Array(items)));
            }

            Ok(None)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower_lsp::lsp_types::{TextEdit, Url};

    #[test]
    fn test_sanitize_completion_items_clamps_out_of_range_positions() {
        let uri = Url::parse("file:///tmp/test.graphql").expect("valid test uri");
        let doc = DocumentState::new_from_thread_local(
            uri,
            "query {\n  users\n}\n",
            PositionEncodingKind::UTF16,
        );

        let mut items = vec![CompletionItem {
            label: "users".to_string(),
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range: Range::new(Position::new(9, 500), Position::new(10, 600)),
                new_text: "users".to_string(),
            })),
            additional_text_edits: Some(vec![TextEdit {
                range: Range::new(Position::new(1, 999), Position::new(1, 1000)),
                new_text: "x".to_string(),
            }]),
            ..Default::default()
        }];

        sanitize_completion_items(&doc, &mut items);

        let item = &items[0];
        let CompletionTextEdit::Edit(main_edit) = item.text_edit.clone().expect("text edit exists")
        else {
            panic!("expected main edit");
        };
        assert!(main_edit.range.start.line <= 3);
        assert!(main_edit.range.end.line <= 3);
        assert!(
            main_edit.range.start.character <= line_max_character(&doc, main_edit.range.start.line)
        );
        assert!(
            main_edit.range.end.character <= line_max_character(&doc, main_edit.range.end.line)
        );

        let additional = item
            .additional_text_edits
            .as_ref()
            .expect("additional edits exist");
        let extra = &additional[0];
        assert!(extra.range.start.character <= line_max_character(&doc, extra.range.start.line));
        assert!(extra.range.end.character <= line_max_character(&doc, extra.range.end.line));
    }
}
