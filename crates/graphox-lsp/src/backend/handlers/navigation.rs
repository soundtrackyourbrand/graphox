use crate::backend::state::Backend;
use graphox_core::DocumentState;
use graphox_features::definition::DocumentDefinition;
use graphox_features::document_highlight::DocumentHighlightFeature;
use graphox_features::folding_range::DocumentFoldingRange;
use graphox_features::references::DocumentReferences;
use graphox_features::selection_range::DocumentSelectionRange;

use std::sync::Arc;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;

pub async fn handle_goto_definition(
    backend: &Backend,
    params: GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>> {
    backend
        .with_tracing("goto_definition", async move {
            let uri = backend.normalize_uri(params.text_document_position_params.text_document.uri);
            let position = params.text_document_position_params.position;

            let doc_arc = if let Some(d) = backend.documents.get(&uri).map(|r| r.value().clone()) {
                d
            } else {
                return Ok(None);
            };

            let symbol_name = doc_arc.get_symbol_at_position(position);
            let schema = backend.get_schema_for_doc(&uri);

            // 1. Try unified definition lookup using the shared resolver
            let preferred_uris = backend.get_preferred_schema_uris(&uri);
            if let Some(location) =
                doc_arc.get_definition(position, &schema, &backend.documents, &preferred_uris)
            {
                return Ok(Some(GotoDefinitionResponse::Scalar(location)));
            }

            // 2. Fallback to fragment definition (requires fragment index in Backend)
            if let Some(location) = backend
                .try_goto_fragment_definition(&symbol_name, &doc_arc)
                .await
            {
                return Ok(Some(GotoDefinitionResponse::Scalar(location)));
            }

            Ok(None)
        })
        .await
}

pub async fn handle_references(
    backend: &Backend,
    params: ReferenceParams,
) -> Result<Option<Vec<Location>>> {
    backend
        .with_tracing("references", async move {
            let uri =
                backend.normalize_uri(params.text_document_position.text_document.uri.clone());
            let position = params.text_document_position.position;
            let include_declaration = params.context.include_declaration;

            let symbol_name =
                if let Some(doc) = backend.documents.get(&uri).map(|r| r.value().clone()) {
                    doc.get_symbol_at_position(position)
                } else {
                    None
                };

            if let Some(name) = symbol_name {
                if name.starts_with('$') {
                    if let Some(doc) = backend.documents.get(&uri).map(|r| r.value().clone()) {
                        let mut all_refs =
                            doc.find_variable_references(&name, position, include_declaration);

                        // Find transitive references in fragments
                        if let Some((op_node, offset)) =
                            doc.find_containing_operation_node(position)
                        {
                            let initial_spreads = doc.get_fragment_spreads_in_node(op_node, offset);
                            let frag_uris = backend.get_transitive_fragments(
                                initial_spreads,
                                doc.package_root.as_ref(),
                            );

                            for f_uri in frag_uris {
                                if let Some(f_doc) =
                                    backend.documents.get(&f_uri).map(|r| r.value().clone())
                                {
                                    let frag_refs = f_doc.find_references_in_tree(&name, false);
                                    all_refs.extend(frag_refs);
                                }
                            }
                        }

                        return Ok(if all_refs.is_empty() {
                            None
                        } else {
                            Some(all_refs)
                        });
                    }
                    return Ok(None);
                }

                let mut all_references = Vec::new();

                let doc_arcs: Vec<Arc<DocumentState>> = backend
                    .documents
                    .iter()
                    .map(|e| e.value().clone())
                    .collect();

                for other_doc in doc_arcs {
                    let refs = other_doc.find_references_in_tree(&name, include_declaration);
                    all_references.extend(refs);
                }

                if all_references.is_empty() {
                    return Ok(None);
                }

                return Ok(Some(all_references));
            }

            Ok(None)
        })
        .await
}

pub async fn handle_document_highlight(
    backend: &Backend,
    params: DocumentHighlightParams,
) -> Result<Option<Vec<DocumentHighlight>>> {
    backend
        .with_tracing("document_highlight", async move {
            let uri = backend.normalize_uri(
                params
                    .text_document_position_params
                    .text_document
                    .uri
                    .clone(),
            );
            let position = params.text_document_position_params.position;

            if let Some(doc) = backend.documents.get(&uri).map(|r| r.value().clone()) {
                let schema = backend.get_schema_for_doc(&uri);
                return Ok(doc.get_document_highlights(position, &schema));
            }

            Ok(None)
        })
        .await
}

pub async fn handle_rename(
    backend: &Backend,
    params: RenameParams,
) -> Result<Option<WorkspaceEdit>> {
    backend
        .with_tracing("rename", async move {
            let uri =
                backend.normalize_uri(params.text_document_position.text_document.uri.clone());
            let position = params.text_document_position.position;
            let new_name = params.new_name;

            let symbol_name =
                if let Some(doc) = backend.documents.get(&uri).map(|r| r.value().clone()) {
                    doc.get_symbol_at_position(position)
                } else {
                    None
                };

            if let Some(name) = symbol_name {
                let mut changes = std::collections::HashMap::new();

                if name.starts_with('$') {
                    if let Some(doc) = backend.documents.get(&uri).map(|r| r.value().clone()) {
                        let refs = doc.find_variable_references(&name, position, true);
                        if !refs.is_empty() {
                            let edits: Vec<TextEdit> = refs
                                .into_iter()
                                .map(|loc| TextEdit {
                                    range: loc.range,
                                    new_text: new_name.clone(),
                                })
                                .collect();
                            changes.insert(uri.clone(), edits);
                        }
                    }
                    return Ok(if changes.is_empty() {
                        None
                    } else {
                        Some(WorkspaceEdit {
                            changes: Some(changes),
                            ..Default::default()
                        })
                    });
                }

                let mut relevant_uris = std::collections::HashSet::new();

                if let Some(def_uris) = backend.fragment_definitions.get(&*name) {
                    relevant_uris.extend(def_uris.iter().cloned());
                }
                if let Some(dep_uris) = backend.fragment_dependents.get(&*name) {
                    relevant_uris.extend(dep_uris.iter().cloned());
                }

                for other_uri in relevant_uris {
                    if let Some(other_doc) =
                        backend.documents.get(&other_uri).map(|r| r.value().clone())
                    {
                        let refs = other_doc.find_references_in_tree(&name, true);
                        if !refs.is_empty() {
                            let edits: Vec<TextEdit> = refs
                                .into_iter()
                                .map(|loc| TextEdit {
                                    range: loc.range,
                                    new_text: new_name.clone(),
                                })
                                .collect();
                            changes.insert(other_uri.clone(), edits);
                        }
                    }
                }

                return Ok(Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }));
            }

            Ok(None)
        })
        .await
}

pub async fn handle_prepare_rename(
    backend: &Backend,
    params: TextDocumentPositionParams,
) -> Result<Option<PrepareRenameResponse>> {
    backend
        .with_tracing("prepare_rename", async move {
            let uri = backend.normalize_uri(params.text_document.uri.clone());
            let position = params.position;

            if let Some(doc) = backend.documents.get(&uri).map(|r| r.value().clone())
                && let Some(_name) = doc.get_symbol_at_position(position)
            {
                return Ok(Some(PrepareRenameResponse::DefaultBehavior {
                    default_behavior: true,
                }));
            }

            Ok(None)
        })
        .await
}

pub async fn handle_folding_range(
    backend: &Backend,
    params: FoldingRangeParams,
) -> Result<Option<Vec<FoldingRange>>> {
    backend
        .with_tracing("folding_range", async move {
            let uri = backend.normalize_uri(params.text_document.uri);
            if let Some(doc) = backend.documents.get(&uri).map(|r| r.value().clone()) {
                let ranges = doc.get_folding_ranges();
                return Ok(if ranges.is_empty() {
                    None
                } else {
                    Some(ranges)
                });
            }
            Ok(None)
        })
        .await
}

pub async fn handle_selection_range(
    backend: &Backend,
    params: SelectionRangeParams,
) -> Result<Option<Vec<SelectionRange>>> {
    backend
        .with_tracing("selection_range", async move {
            let uri = backend.normalize_uri(params.text_document.uri);
            if let Some(doc) = backend.documents.get(&uri).map(|r| r.value().clone()) {
                let ranges = doc.get_selection_ranges(params.positions);
                return Ok(if ranges.is_empty() {
                    None
                } else {
                    Some(ranges)
                });
            }
            Ok(None)
        })
        .await
}
