use crate::backend::state::Backend;
use graphox_features::call_hierarchy::DocumentCallHierarchy;
use graphox_features::definition::DocumentDefinition;
use graphox_features::references::DocumentReferences;

use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::*;

pub async fn handle_prepare_call_hierarchy(
    backend: &Backend,
    params: CallHierarchyPrepareParams,
) -> Result<Option<Vec<CallHierarchyItem>>> {
    backend
        .with_tracing("prepare_call_hierarchy", async move {
            let uri = backend.normalize_uri(
                params
                    .text_document_position_params
                    .text_document
                    .uri
                    .clone(),
            );
            let position = params.text_document_position_params.position;

            if let Some(doc) = backend.documents.get(&uri).map(|r| r.value().clone()) {
                return Ok(doc.prepare_call_hierarchy(position));
            }

            Ok(None)
        })
        .await
}

pub async fn handle_incoming_calls(
    backend: &Backend,
    params: CallHierarchyIncomingCallsParams,
) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
    backend
        .with_tracing("incoming_calls", async move {
            let item = params.item;
            let symbol_name = item.name;
            let mut incoming = Vec::new();

            let dependent_uris = backend
                .fragment_dependents
                .get(&*symbol_name)
                .map(|set| set.iter().cloned().collect::<Vec<_>>());

            if let Some(uris) = dependent_uris {
                for dep_uri in uris {
                    if let Some(doc) = backend.documents.get(&dep_uri).map(|r| r.value().clone()) {
                        let refs = doc.find_references_in_tree(&symbol_name, false);

                        if !refs.is_empty() {
                            let mut ranges_by_container: std::collections::HashMap<
                                String,
                                Vec<Range>,
                            > = std::collections::HashMap::new();

                            for r in refs {
                                let container_name = doc.get_container_name_at_range(r.range);
                                let key = container_name.unwrap_or_else(|| "unknown".to_string());
                                ranges_by_container.entry(key).or_default().push(r.range);
                            }

                            for (name, ranges) in ranges_by_container {
                                incoming.push(CallHierarchyIncomingCall {
                                    from: CallHierarchyItem {
                                        name: name.clone(),
                                        kind: SymbolKind::FUNCTION,
                                        tags: None,
                                        detail: Some(doc.uri.to_string()),
                                        uri: doc.uri.clone(),
                                        range: doc
                                            .find_definition_in_tree(&name)
                                            .map(|l| l.range)
                                            .unwrap_or(ranges[0]),
                                        selection_range: doc
                                            .find_definition_in_tree(&name)
                                            .map(|l| l.range)
                                            .unwrap_or(ranges[0]),
                                        data: None,
                                    },
                                    from_ranges: ranges,
                                });
                            }
                        }
                    }
                }
            }

            if incoming.is_empty() {
                Ok(None)
            } else {
                Ok(Some(incoming))
            }
        })
        .await
}

pub async fn handle_outgoing_calls(
    backend: &Backend,
    params: CallHierarchyOutgoingCallsParams,
) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
    backend
        .with_tracing("outgoing_calls", async move {
            let item = params.item;
            let symbol_name = item.name;
            let uri = backend.normalize_uri(item.uri);

            if let Some(doc) = backend.documents.get(&uri).map(|r| r.value().clone()) {
                let mut calls = doc.get_outgoing_calls(&symbol_name);

                for call in &mut calls {
                    let callee_name = call.to.name.clone();
                    if let Some(def_uris) = backend.fragment_definitions.get(callee_name.as_str()) {
                        for def_uri in def_uris.iter() {
                            if let Some(def_doc) =
                                backend.documents.get(def_uri).map(|r| r.value().clone())
                                && let Some(loc) = def_doc.find_definition_in_tree(&callee_name)
                            {
                                call.to.uri = loc.uri;
                                call.to.range = loc.range;
                                call.to.selection_range = loc.range;
                                break;
                            }
                        }
                    }
                }

                return Ok(Some(calls));
            }

            Ok(None)
        })
        .await
}
