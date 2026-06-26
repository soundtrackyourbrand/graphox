//! Fragment metadata collection and management
//!
//! This module extracts the fragment-related logic that was duplicated in
//! Backend::initialized and Backend::get_all_fragments_info

use ahash::{AHashMap, AHashSet};
use dashmap::DashMap;
use graphox_core::config::Config;
use graphox_core::document::DocumentState;
use graphox_core::schema::{SloClass, SubgraphInfo};
use graphox_core::types::MetadataMap;
use graphox_features::completion::FragmentCompletionInfo;
use std::sync::Arc;
use tower_lsp::lsp_types::Url;
use tree_sitter::StreamingIterator;

/// Collects fragment metadata from fragment definitions
use graphox_core::queries::GQL_SYMBOL_QUERY_CACHE;
use tree_sitter::QueryCursor;

fn extract_fragment_nodes(doc: &DocumentState) -> AHashMap<String, (tree_sitter::Node<'_>, usize)> {
    let mut fragment_nodes = AHashMap::default();
    for block in doc.get_graphql_trees() {
        let query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, graphox_core::queries::GQL_SYMBOL_QUERY).unwrap()
        });

        let mut cursor = QueryCursor::new();
        let mut matches =
            cursor.matches(query, block.tree.root_node(), |node: tree_sitter::Node| {
                doc.rope
                    .byte_slice(
                        (node.start_byte() + block.offset)..(node.end_byte() + block.offset),
                    )
                    .chunks()
            });

        while let Some(m) = matches.next() {
            let node = m.captures[0].node;
            if node.kind() == "fragment_definition"
                && let Some(name) = doc
                    .find_child_by_kind(node, "fragment_name")
                    .and_then(|n| doc.find_child_by_kind(n, "name"))
                    .map(|n| doc.get_node_text(n, block.offset))
            {
                fragment_nodes.insert(name, (node, block.offset));
            }
        }
    }
    fragment_nodes
}

#[allow(clippy::too_many_arguments)]
fn compute_worst_slo_for_fragment(
    frag: &graphox_core::document::FragmentDef,
    doc_clone: &Option<Arc<DocumentState>>,
    schema: &apollo_compiler::Schema,
    project_subgraphs: &[SubgraphInfo],
    fragment_nodes: &AHashMap<String, (tree_sitter::Node<'_>, usize)>,
    documents: &Arc<DashMap<Url, Arc<DocumentState>, ahash::RandomState>>,
    uri: &Url,
    fragment_index: &ahash::AHashMap<
        Arc<str>,
        Vec<(Arc<DocumentState>, graphox_core::document::FragmentDef)>,
    >,
) -> Option<SloClass> {
    let mut worst_slo: Option<SloClass> = None;

    if let Some((node, offset)) = fragment_nodes.get(frag.name.as_ref())
        && let Some(selection) = doc_clone
            .as_ref()
            .and_then(|d| d.find_child_by_kind(*node, "selection_set"))
        && let Some(type_cond) = schema.types.get(frag.type_condition.as_ref())
    {
        use graphox_features::hover::DocumentHover;
        let mut visited = ahash::AHashSet::default();
        visited.insert(frag.name.clone());
        if let Some(doc) = documents.get(uri).map(|r| r.value().clone()) {
            let slo = doc.calculate_worst_slo_for_selection_set(
                selection,
                *offset,
                type_cond,
                schema,
                project_subgraphs,
                fragment_index,
                documents,
                &mut visited,
            );
            if let Some(s) = slo {
                worst_slo = Some(worst_slo.map_or(s, |w| w.worst(s)));
            }
        }
    }
    worst_slo
}

/// Collect fragment metadata for the whole workspace.
///
/// `compute_slo` controls whether each fragment's worst-case SLO is computed.
/// SLO computation requires a tree-sitter pass over every document
/// (`extract_fragment_nodes`) plus a workspace-wide fragment index, which is
/// pure overhead on the validation hot path (diagnostics never read `worst_slo`).
/// Only the completion/hover path (`get_all_fragments_info`) needs it, so callers
/// on the per-edit validation path pass `false` to skip that work — mirroring what
/// `graphox check` does.
pub fn collect_fragment_metadata(
    metadata: &MetadataMap,
    config: &Config,
    subgraphs: &Arc<DashMap<String, Vec<SubgraphInfo>, ahash::RandomState>>,
    documents: &Arc<DashMap<Url, Arc<DocumentState>, ahash::RandomState>>,
    schemas: &Arc<DashMap<String, Arc<apollo_compiler::Schema>, ahash::RandomState>>,
    compute_slo: bool,
) -> Vec<FragmentCompletionInfo> {
    // Clone Arc references to avoid holding locks during iteration
    let metadata_arc = metadata.clone();
    let subgraphs_arc = subgraphs.clone();
    let documents_arc = documents.clone();
    let schemas_arc = schemas.clone();

    // Build fragment index ONCE — only needed for SLO computation.
    let mut fragment_index = ahash::AHashMap::default();
    if compute_slo {
        for entry in documents_arc.iter() {
            let d = entry.value();
            for f in d.fragments() {
                fragment_index
                    .entry(f.name.clone())
                    .or_insert_with(Vec::new)
                    .push((d.clone(), f.clone()));
            }
        }
    }

    metadata_arc
        .iter()
        .flat_map(|entry| {
            let uri = entry.key();
            let meta = entry.value();

            let Some(path) = super::validation::get_configured_document_path(uri, config) else {
                return Vec::<FragmentCompletionInfo>::new();
            };
            let Some(project) = config.get_project_for_path(&path) else {
                return Vec::<FragmentCompletionInfo>::new();
            };
            let schema_key = project.schema().as_key();
            let import_path = project.import().map(|s| s.to_string());
            let project_subgraphs = if compute_slo {
                subgraphs_arc
                    .get(schema_key.as_str())
                    .map(|r| r.value().clone())
            } else {
                None
            };
            let schema = if compute_slo {
                schemas_arc
                    .get(schema_key.as_str())
                    .map(|r| r.value().clone())
            } else {
                None
            };

            let doc = documents_arc.get(uri).map(|r| r.value().clone());
            // `extract_fragment_nodes` runs a tree-sitter query per document; only
            // worth it when we're going to compute SLO.
            let fragment_nodes = if compute_slo {
                doc.as_deref()
                    .map(extract_fragment_nodes)
                    .unwrap_or_default()
            } else {
                AHashMap::default()
            };

            let documents_clone = documents_arc.clone();
            let uri_clone = uri.clone();
            let doc_clone = doc.clone();
            let project_subgraphs_clone: Option<Vec<SubgraphInfo>> = project_subgraphs;
            let schema_clone: Option<Arc<apollo_compiler::Schema>> = schema;
            let fragment_index_ref = &fragment_index;

            meta.fragments
                .iter()
                .map(move |frag| {
                    let worst_slo = if let (Some(schema), Some(subgraphs)) =
                        (schema_clone.as_ref(), project_subgraphs_clone.as_ref())
                    {
                        compute_worst_slo_for_fragment(
                            frag,
                            &doc_clone,
                            schema,
                            subgraphs,
                            &fragment_nodes,
                            &documents_clone,
                            &uri_clone,
                            fragment_index_ref,
                        )
                    } else {
                        None
                    };

                    FragmentCompletionInfo {
                        name: frag.name.clone(),
                        type_condition: frag.type_condition.clone(),
                        description: frag.description.clone(),
                        import_path: import_path.as_deref().map(|s: &str| Arc::from(s)),
                        is_public: frag.is_public,
                        is_type_only: frag.is_type_only,
                        uri: uri_clone.clone(),
                        package_root: meta.package_root.clone(),
                        used_variables: frag.used_variables.clone(),
                        used_fragments: frag.used_fragments.clone(),
                        transitive_deps: frag.transitive_deps.clone(),
                        selected_fields: frag.selected_fields.clone(),
                        type_fields: frag.type_fields.clone(),
                        requirements: std::collections::BTreeMap::new(),
                        worst_slo,
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Like [`collect_fragment_metadata`], but also returns each fragment's schema key.
///
/// See [`collect_fragment_metadata`] for the meaning of `compute_slo`.
pub fn collect_fragment_metadata_with_schema(
    metadata: &MetadataMap,
    config: &Config,
    subgraphs: &Arc<DashMap<String, Vec<SubgraphInfo>, ahash::RandomState>>,
    documents: &Arc<DashMap<Url, Arc<DocumentState>, ahash::RandomState>>,
    schemas: &Arc<DashMap<String, Arc<apollo_compiler::Schema>, ahash::RandomState>>,
    compute_slo: bool,
) -> Vec<(FragmentCompletionInfo, Option<Arc<str>>)> {
    // Clone Arc references to avoid holding locks during iteration
    let metadata_arc = metadata.clone();
    let subgraphs_arc = subgraphs.clone();
    let documents_arc = documents.clone();
    let schemas_arc = schemas.clone();

    // Build fragment index ONCE — only needed for SLO computation.
    let mut fragment_index = ahash::AHashMap::default();
    if compute_slo {
        for entry in documents_arc.iter() {
            let d = entry.value();
            for f in d.fragments() {
                fragment_index
                    .entry(f.name.clone())
                    .or_insert_with(Vec::new)
                    .push((d.clone(), f.clone()));
            }
        }
    }

    metadata_arc
        .iter()
        .flat_map(|entry| {
            let uri = entry.key();
            let meta = entry.value();

            let Some(path) = super::validation::get_configured_document_path(uri, config) else {
                return Vec::<(FragmentCompletionInfo, Option<Arc<str>>)>::new();
            };
            let Some(project) = config.get_project_for_path(&path) else {
                return Vec::<(FragmentCompletionInfo, Option<Arc<str>>)>::new();
            };
            let schema_key = Some(Arc::from(project.schema().as_key()));
            let import_path = project.import().map(Arc::from);
            let project_subgraphs = if compute_slo {
                schema_key
                    .as_ref()
                    .and_then(|k: &Arc<str>| subgraphs_arc.get::<str>(k.as_ref()))
                    .map(|r| r.value().clone())
            } else {
                None
            };
            let schema = if compute_slo {
                schema_key
                    .as_ref()
                    .and_then(|k: &Arc<str>| schemas_arc.get::<str>(k.as_ref()))
                    .map(|r| r.value().clone())
            } else {
                None
            };

            let doc = documents_arc.get(uri).map(|r| r.value().clone());
            // `extract_fragment_nodes` runs a tree-sitter query per document; only
            // worth it when we're going to compute SLO.
            let fragment_nodes = if compute_slo {
                doc.as_deref()
                    .map(extract_fragment_nodes)
                    .unwrap_or_default()
            } else {
                AHashMap::default()
            };

            let documents_clone = documents_arc.clone();
            let uri_clone = uri.clone();
            let doc_clone = doc.clone();
            let project_subgraphs_clone: Option<Vec<SubgraphInfo>> = project_subgraphs;
            let schema_clone: Option<Arc<apollo_compiler::Schema>> = schema;
            let schema_key_clone: Option<Arc<str>> = schema_key;
            let fragment_index_ref = &fragment_index;

            meta.fragments
                .iter()
                .map(move |frag| {
                    let worst_slo = if let (Some(schema), Some(subgraphs)) =
                        (schema_clone.as_ref(), project_subgraphs_clone.as_ref())
                    {
                        compute_worst_slo_for_fragment(
                            frag,
                            &doc_clone,
                            schema,
                            subgraphs,
                            &fragment_nodes,
                            &documents_clone,
                            &uri_clone,
                            fragment_index_ref,
                        )
                    } else {
                        None
                    };

                    (
                        FragmentCompletionInfo {
                            name: frag.name.clone(),
                            type_condition: frag.type_condition.clone(),
                            description: frag.description.clone(),
                            import_path: import_path.clone(),
                            is_public: frag.is_public,
                            is_type_only: frag.is_type_only,
                            uri: uri_clone.clone(),
                            package_root: meta.package_root.clone(),
                            used_variables: frag.used_variables.clone(),
                            used_fragments: frag.used_fragments.clone(),
                            transitive_deps: frag.transitive_deps.clone(),
                            selected_fields: frag.selected_fields.clone(),
                            type_fields: frag.type_fields.clone(),
                            requirements: std::collections::BTreeMap::new(),
                            worst_slo,
                        },
                        schema_key_clone.clone(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Updates the fragment dependent index when fragments change
pub fn update_fragment_dependents(
    fragment_dependents: &Arc<DashMap<Arc<str>, AHashSet<Url>, ahash::RandomState>>,
    uri: &Url,
    old_spreads: Option<Arc<[Arc<str>]>>,
    new_spreads: Arc<[Arc<str>]>,
) {
    if let Some(old) = old_spreads {
        for spread in old.iter() {
            if !new_spreads.contains(spread)
                && let Some(mut entry) = fragment_dependents.get_mut(spread)
            {
                entry.value_mut().remove(uri);
            }
        }
    }

    for spread in new_spreads.iter() {
        fragment_dependents
            .entry(spread.clone())
            .or_default()
            .insert(uri.clone());
    }
}

/// Updates the fragment definition index when fragments are added/removed
pub fn update_fragment_definitions(
    fragment_definitions: &Arc<DashMap<Arc<str>, AHashSet<Url>, ahash::RandomState>>,
    uri: &Url,
    old_fragments: Option<Arc<[Arc<str>]>>,
    new_fragments: Arc<[Arc<str>]>,
) {
    if let Some(old) = old_fragments {
        for name in old.iter() {
            if !new_fragments.contains(name)
                && let Some(mut entry) = fragment_definitions.get_mut(name)
            {
                entry.value_mut().remove(uri);
            }
        }
    }

    for name in new_fragments.iter() {
        fragment_definitions
            .entry(name.clone())
            .or_default()
            .insert(uri.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::state::Backend;
    use graphox_core::config::{GlobPattern, ProjectConfig, SchemaSource};
    use tower_lsp::LspService;
    use tower_lsp::lsp_types::PositionEncodingKind;

    /// `compute_slo = false` (the validation hot path) must return exactly the same
    /// fragments as `compute_slo = true`; it may only differ by leaving `worst_slo`
    /// unset. This guards the optimisation against accidentally dropping fragments.
    #[tokio::test]
    #[ntest::timeout(5000)]
    async fn compute_slo_flag_only_affects_worst_slo() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        std::fs::write(
            base.join("schema.graphql"),
            "type Query { user: User } type User { id: ID! name: String }",
        )
        .unwrap();

        let config = Config::new_test(
            base.clone(),
            vec![
                ProjectConfig::default()
                    .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                    .with_include(GlobPattern::Single("**/*.graphql".to_string())),
            ],
        );

        let (service, _) = LspService::new(|client| Backend::new(client, config.clone()));
        let backend = service.inner();

        for i in 0..5 {
            let uri = Url::from_file_path(base.join(format!("doc_{i}.graphql"))).unwrap();
            let content = format!(
                "query Q{i} {{ user {{ ...Frag{i} }} }}\nfragment Frag{i} on User {{ id name }}\n"
            );
            let doc = DocumentState::new_from_thread_local(
                uri.clone(),
                &content,
                PositionEncodingKind::UTF16,
            );
            let metadata = Arc::new(graphox_core::types::DocumentMetadata {
                fragments: doc.fragments.clone(),
                fragment_spreads: doc.fragment_spreads.clone(),
                package_root: doc.package_root.clone(),
                operations: doc.operations.clone(),
                version: 0,
            });
            backend.documents.insert(uri.clone(), Arc::new(doc));
            backend.metadata.insert(uri, metadata);
        }

        let mut names_no_slo: Vec<String> = collect_fragment_metadata(
            &backend.metadata,
            &config,
            &backend.subgraphs,
            &backend.documents,
            &backend.schemas,
            false,
        )
        .into_iter()
        .map(|f| {
            assert!(
                f.worst_slo.is_none(),
                "no-SLO mode must not populate worst_slo"
            );
            f.name.to_string()
        })
        .collect();

        let mut names_with_slo: Vec<String> = collect_fragment_metadata(
            &backend.metadata,
            &config,
            &backend.subgraphs,
            &backend.documents,
            &backend.schemas,
            true,
        )
        .into_iter()
        .map(|f| f.name.to_string())
        .collect();

        names_no_slo.sort();
        names_with_slo.sort();
        assert_eq!(
            names_no_slo, names_with_slo,
            "both modes must surface the same fragments"
        );
        assert_eq!(names_no_slo.len(), 5);
    }
}
