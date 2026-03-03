use crate::backend::state::Backend;
use graphox_core::DocumentState;
use graphox_features::semantic_tokens::DocumentSemanticTokens;
use graphox_features::symbols::DocumentSymbols;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;

/// Finds a symbol by exact name match within a document.
/// This is more efficient than get_symbols() when we already know the name.
fn find_symbol_by_name(doc: &DocumentState, name: &str) -> Option<DocumentSymbol> {
    doc.get_symbols().into_iter().find(|sym| sym.name == name)
}

fn extended_type_to_symbol_kind(ty: &apollo_compiler::schema::ExtendedType) -> SymbolKind {
    match ty {
        apollo_compiler::schema::ExtendedType::Object(_) => SymbolKind::CLASS,
        apollo_compiler::schema::ExtendedType::Interface(_) => SymbolKind::INTERFACE,
        apollo_compiler::schema::ExtendedType::Enum(_) => SymbolKind::ENUM,
        apollo_compiler::schema::ExtendedType::Scalar(_) => SymbolKind::TYPE_PARAMETER,
        apollo_compiler::schema::ExtendedType::Union(_) => SymbolKind::ENUM,
        apollo_compiler::schema::ExtendedType::InputObject(_) => SymbolKind::CLASS,
    }
}

pub async fn handle_document_symbol(
    backend: &Backend,
    params: DocumentSymbolParams,
) -> Result<Option<DocumentSymbolResponse>> {
    backend
        .with_tracing("document_symbol", async move {
            let uri = backend.normalize_uri(params.text_document.uri);
            if let Some(doc) = backend.documents.get(&uri).map(|r| r.value().clone()) {
                let symbols = doc.get_symbols();
                return Ok(Some(DocumentSymbolResponse::Nested(symbols)));
            }
            Ok(None)
        })
        .await
}

pub async fn handle_workspace_symbol(
    backend: &Backend,
    params: WorkspaceSymbolParams,
) -> Result<Option<Vec<SymbolInformation>>> {
    backend
        .with_tracing("workspace_symbol", async move {
            let query = params.query.to_lowercase();
            let mut all_symbols = Vec::new();

            // Phase 1: Query fragment_definitions index for matching fragments
            // This is O(n) where n = number of unique fragment names (much smaller than all documents)
            for entry in backend.fragment_definitions.iter() {
                let frag_name = entry.key();
                if frag_name.to_lowercase().contains(&query) {
                    let urls = entry.value();
                    // Fetch each document that contains this fragment
                    for url in urls {
                        let doc_arc = backend.load_doc_from_cache_or_disk(url).await;
                        if let Some(doc) = doc_arc {
                            // Find the specific fragment symbol
                            if let Some(sym) = find_symbol_by_name(&doc, frag_name) {
                                #[allow(deprecated)]
                                all_symbols.push(SymbolInformation {
                                    name: sym.name,
                                    kind: sym.kind,
                                    tags: sym.tags,
                                    deprecated: sym.deprecated,
                                    location: Location {
                                        uri: doc.uri.clone(),
                                        range: sym.selection_range,
                                    },
                                    container_name: sym.detail,
                                });
                            }
                        }
                    }
                }
            }

            // Phase 2: Query operation_names index for matching operations
            // operation_names maps: name -> Vec<(schema_key, Url)>
            for entry in backend.operation_names.iter() {
                let op_name = entry.key();
                if op_name.to_lowercase().contains(&query) {
                    let occurrences = entry.value();
                    // Each occurrence is (schema_key, url) - collect unique URLs
                    for (_, url) in occurrences {
                        let doc_arc = backend.load_doc_from_cache_or_disk(url).await;
                        if let Some(doc) = doc_arc {
                            // Find the specific operation symbol
                            if let Some(sym) = find_symbol_by_name(&doc, op_name) {
                                #[allow(deprecated)]
                                all_symbols.push(SymbolInformation {
                                    name: sym.name,
                                    kind: sym.kind,
                                    tags: sym.tags,
                                    deprecated: sym.deprecated,
                                    location: Location {
                                        uri: doc.uri.clone(),
                                        range: sym.selection_range,
                                    },
                                    container_name: sym.detail,
                                });
                            }
                        }
                    }
                }
            }

            // Phase 3: Query schemas for matching types
            for entry in backend.schemas.iter() {
                let schema = entry.value();
                for (name, ty) in &schema.types {
                    if name.to_lowercase().contains(&query) {
                        // Find which file defines this type
                        // For now, we can use the first file in the schema that defines it
                        // Or we can just use the schema key to find the file
                        if let Some(loc) = backend.find_type_definition_in_schema(schema, name) {
                            #[allow(deprecated)]
                            all_symbols.push(SymbolInformation {
                                name: name.to_string(),
                                kind: extended_type_to_symbol_kind(ty),
                                tags: None,
                                deprecated: None,
                                location: loc,
                                container_name: None,
                            });
                        }
                    }
                }
            }

            // Phase 4: Query subgraphs for matching types
            for entry in backend.subgraphs.iter() {
                let subgraph_infos = entry.value();
                for info in subgraph_infos {
                    for (name, ty) in &info.schema.types {
                        if name.to_lowercase().contains(&query)
                            && let Some(loc) =
                                backend.find_type_definition_in_schema(&info.schema, name)
                        {
                            #[allow(deprecated)]
                            all_symbols.push(SymbolInformation {
                                name: name.to_string(),
                                kind: extended_type_to_symbol_kind(ty),
                                tags: None,
                                deprecated: None,
                                location: loc,
                                container_name: Some(format!("[{}]", info.name)),
                            });
                        }
                    }
                }
            }

            if all_symbols.is_empty() {
                Ok(None)
            } else {
                Ok(Some(all_symbols))
            }
        })
        .await
}

pub async fn handle_semantic_tokens_full(
    backend: &Backend,
    params: SemanticTokensParams,
) -> Result<Option<SemanticTokensResult>> {
    backend
        .with_tracing("semantic_tokens_full", async move {
            let uri = backend.normalize_uri(params.text_document.uri);
            if let Some(doc) = backend.documents.get(&uri).map(|r| r.value().clone()) {
                let tokens = doc.get_semantic_tokens();
                return Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
                    result_id: None,
                    data: tokens,
                })));
            }
            Ok(None)
        })
        .await
}
