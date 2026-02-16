use crate::backend::state::Backend;
use apollo_compiler::Schema;
use graphox_features::definition::DocumentDefinition;
use graphox_features::document_highlight::DocumentHighlightFeature;
use graphox_features::folding_range::DocumentFoldingRange;
use graphox_features::references::DocumentReferences;
use graphox_features::selection_range::DocumentSelectionRange;
use graphox_features::shared::type_resolver::{self, SemanticSymbol};
use graphox_features::type_definition::DocumentTypeDefinition;
use rayon::prelude::*;

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

            let doc = if let Some(d) = backend.documents.get(&uri).map(|r| r.value().clone()) {
                d
            } else {
                return Ok(None);
            };

            let schema = backend.get_schema_for_doc(&uri);
            let symbol_name = doc.get_symbol_at_position(position);

            // Try to resolve the semantic symbol at position for type-aware references
            if let Some(resolved) = resolve_symbol_at_position(&doc, position, &schema) {
                match resolved {
                    ResolvedSymbol::Field {
                        field_name,
                        parent_type_name,
                    } => {
                        return Ok(find_field_references_across_workspace(
                            backend,
                            &field_name,
                            &parent_type_name,
                            &schema,
                            include_declaration,
                        ));
                    }
                    ResolvedSymbol::Directive { name } => {
                        return Ok(find_directive_references_across_workspace(
                            backend,
                            &name,
                            include_declaration,
                        ));
                    }
                    ResolvedSymbol::Variable { name } => {
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
                    _ => {}
                }
            }

            // Fallback to name-based references for other symbol types
            if let Some(name) = symbol_name {
                if name.starts_with('$') {
                    let mut all_refs =
                        doc.find_variable_references(&name, position, include_declaration);

                    // Find transitive references in fragments
                    if let Some((op_node, offset)) = doc.find_containing_operation_node(position) {
                        let initial_spreads = doc.get_fragment_spreads_in_node(op_node, offset);
                        let frag_uris = backend
                            .get_transitive_fragments(initial_spreads, doc.package_root.as_ref());

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

                let mut relevant_uris = std::collections::HashSet::new();

                if let Some(def_uris) = backend.fragment_definitions.get(&*name) {
                    for u in def_uris.iter() {
                        relevant_uris.insert(u.clone());
                    }
                }
                if let Some(dep_uris) = backend.fragment_dependents.get(&*name) {
                    for u in dep_uris.iter() {
                        relevant_uris.insert(u.clone());
                    }
                }

                let all_references: Vec<Location> = if relevant_uris.is_empty() {
                    // Fallback to full scan if not found in fragment indices (could be a type/field/etc)
                    backend
                        .documents
                        .iter()
                        .par_bridge()
                        .flat_map(|entry| {
                            entry
                                .value()
                                .find_references_in_tree(&name, include_declaration)
                        })
                        .collect()
                } else {
                    relevant_uris
                        .iter()
                        .par_bridge()
                        .filter_map(|u| backend.documents.get(u))
                        .flat_map(|entry| {
                            entry
                                .value()
                                .find_references_in_tree(&name, include_declaration)
                        })
                        .collect()
                };

                if all_references.is_empty() {
                    return Ok(None);
                }

                return Ok(Some(all_references));
            }

            Ok(None)
        })
        .await
}

/// Resolved symbol for reference finding purposes
#[allow(dead_code)]
enum ResolvedSymbol {
    Field {
        field_name: String,
        parent_type_name: String,
    },
    Variable {
        name: String,
    },
    Fragment {
        name: String,
    },
    Type {
        name: String,
    },
    Directive {
        name: String,
    },
    EnumValue {
        enum_name: String,
        value_name: String,
    },
    Argument {
        parent_type_name: String,
        field_name: Option<String>,
        arg_name: String,
    },
    InputField {
        parent_type_name: String,
        field_name: String,
    },
    Other,
}

fn resolve_symbol_at_position(
    doc: &graphox_core::document::DocumentState,
    position: Position,
    schema: &Schema,
) -> Option<ResolvedSymbol> {
    let cursor_offset = doc.position_to_byte(position);

    for block in doc.get_graphql_trees() {
        let offset = block.offset;
        let root = block.tree.root_node();
        let tree_len = root.end_byte();

        if cursor_offset >= offset && cursor_offset < offset + tree_len {
            let local_byte = cursor_offset - offset;
            let node = root.descendant_for_byte_range(local_byte, local_byte)?;

            // First check if we're on a field_definition in a schema (field definitions)
            if node.kind() == "name"
                && let Some(parent) = node.parent()
                && parent.kind() == "field_definition"
            {
                // We're on a field name in a field_definition
                // Find the containing type
                let field_name = doc.get_node_text(node, offset);
                let parent_type = find_containing_type_for_field_def(parent, doc, offset)?;

                return Some(ResolvedSymbol::Field {
                    field_name,
                    parent_type_name: parent_type,
                });
            }

            // Then try the standard semantic symbol resolution
            let symbol =
                type_resolver::resolve_symbol_at_node(doc, node, offset, cursor_offset, schema)?;

            return Some(match symbol {
                SemanticSymbol::Field {
                    parent_type,
                    field_def,
                    ..
                } => ResolvedSymbol::Field {
                    field_name: field_def.name.to_string(),
                    parent_type_name: parent_type.name().to_string(),
                },
                SemanticSymbol::BuiltinField { name, parent_type } => ResolvedSymbol::Field {
                    field_name: name,
                    parent_type_name: parent_type.name().to_string(),
                },
                SemanticSymbol::Variable { name, .. } => ResolvedSymbol::Variable { name },
                SemanticSymbol::Fragment { name, .. } => ResolvedSymbol::Fragment { name },
                SemanticSymbol::Type(ty) => ResolvedSymbol::Type {
                    name: ty.name().to_string(),
                },
                SemanticSymbol::Directive { dir_def } => ResolvedSymbol::Directive {
                    name: dir_def.name.to_string(),
                },
                SemanticSymbol::EnumValue { enum_name, val_def } => ResolvedSymbol::EnumValue {
                    enum_name,
                    value_name: val_def.value.to_string(),
                },
                SemanticSymbol::Argument {
                    parent_type_name,
                    field_name,
                    arg_def,
                } => ResolvedSymbol::Argument {
                    parent_type_name,
                    field_name,
                    arg_name: arg_def.name.to_string(),
                },
                SemanticSymbol::InputObjectField {
                    parent_type,
                    field_def,
                } => ResolvedSymbol::InputField {
                    parent_type_name: parent_type.name().to_string(),
                    field_name: field_def.name.to_string(),
                },
                _ => ResolvedSymbol::Other,
            });
        }
    }
    None
}

/// Find the parent type name for a field_definition node
fn find_containing_type_for_field_def(
    node: tree_sitter::Node,
    doc: &graphox_core::document::DocumentState,
    offset: usize,
) -> Option<String> {
    let mut curr = node;
    while let Some(parent) = curr.parent() {
        match parent.kind() {
            "object_type_definition" | "interface_type_definition" => {
                if let Some(name_node) = doc.find_child_by_kind(parent, "name") {
                    return Some(doc.get_node_text(name_node, offset));
                }
            }
            _ => {
                curr = parent;
            }
        }
    }
    None
}

fn find_field_references_across_workspace(
    backend: &Backend,
    field_name: &str,
    parent_type_name: &str,
    schema: &Schema,
    include_declaration: bool,
) -> Option<Vec<Location>> {
    let all_references: Vec<Location> = backend
        .documents
        .iter()
        .par_bridge()
        .flat_map(|entry| {
            entry.value().find_field_references(
                field_name,
                parent_type_name,
                schema,
                include_declaration,
            )
        })
        .collect();

    if all_references.is_empty() {
        None
    } else {
        Some(all_references)
    }
}

fn find_directive_references_across_workspace(
    backend: &Backend,
    directive_name: &str,
    include_declaration: bool,
) -> Option<Vec<Location>> {
    let all_references: Vec<Location> = backend
        .documents
        .iter()
        .par_bridge()
        .flat_map(|entry| {
            entry
                .value()
                .find_directive_references(directive_name, include_declaration)
        })
        .collect();

    if all_references.is_empty() {
        None
    } else {
        Some(all_references)
    }
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
                    for uri in def_uris.iter() {
                        relevant_uris.insert(uri.clone());
                    }
                }
                if let Some(dep_uris) = backend.fragment_dependents.get(&*name) {
                    for uri in dep_uris.iter() {
                        relevant_uris.insert(uri.clone());
                    }
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

pub async fn handle_goto_type_definition(
    backend: &Backend,
    params: GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>> {
    backend
        .with_tracing("goto_type_definition", async move {
            let uri = backend.normalize_uri(params.text_document_position_params.text_document.uri);
            let position = params.text_document_position_params.position;

            let doc_arc = if let Some(d) = backend.documents.get(&uri).map(|r| r.value().clone()) {
                d
            } else {
                return Ok(None);
            };

            let schema = backend.get_schema_for_doc(&uri);

            // 1. Try to get the generated type definition
            if let Some(location) = {
                let config = backend.config.read().unwrap();
                doc_arc.get_type_definition(position, &schema, &config)
            } {
                return Ok(Some(GotoDefinitionResponse::Scalar(location)));
            }

            // 2. Fallback: If it's a fragment spread, jump to the definition in the fragment's codegen
            let symbol_name = doc_arc.get_symbol_at_position(position);
            if let Some(location) = backend
                .try_goto_fragment_definition(&symbol_name, &doc_arc)
                .await
            {
                let frag_uri = location.uri.clone();
                if let Some(frag_doc) = backend.documents.get(&frag_uri).map(|r| r.value().clone())
                {
                    let frag_schema = backend.get_schema_for_doc(&frag_uri);
                    // Use the fragment's range start to find its type definition
                    let config = backend.config.read().unwrap();
                    if let Some(type_location) =
                        frag_doc.get_type_definition(location.range.start, &frag_schema, &config)
                    {
                        return Ok(Some(GotoDefinitionResponse::Scalar(type_location)));
                    }
                }
            }

            Ok(None)
        })
        .await
}
