//! Fragment metadata collection and management
//!
//! This module extracts the fragment-related logic that was duplicated in
//! Backend::initialized and Backend::get_all_fragments_info

use ahash::{AHashMap, AHashSet};
use dashmap::DashMap;
use graphox_core::config::Config;
use graphox_core::document::{DocumentState, FragmentDef};
use graphox_core::schema::{SloClass, SubgraphInfo};
use graphox_features::completion::FragmentCompletionInfo;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp::lsp_types::Url;
use tree_sitter::StreamingIterator;

/// Collects fragment metadata from fragment definitions
pub fn collect_fragment_metadata(
    fragment_defs: &Arc<DashMap<Url, Vec<FragmentDef>, ahash::RandomState>>,
    config: &Config,
    package_roots: &Arc<DashMap<Url, Option<PathBuf>, ahash::RandomState>>,
    subgraphs: &Arc<DashMap<String, Vec<SubgraphInfo>, ahash::RandomState>>,
    documents: &Arc<DashMap<Url, Arc<DocumentState>, ahash::RandomState>>,
    schemas: &Arc<DashMap<String, Arc<apollo_compiler::Schema>, ahash::RandomState>>,
) -> Vec<FragmentCompletionInfo> {
    // Clone Arc references to avoid holding locks during iteration
    let fragment_defs = fragment_defs.clone();
    let package_roots = package_roots.clone();
    let subgraphs = subgraphs.clone();
    let documents = documents.clone();
    let schemas = schemas.clone();

    fragment_defs
        .iter()
        .flat_map(|entry| {
            let uri = entry.key();
            let frags = entry.value();

            // Get project info once per file
            let (import_path, package_root, project_subgraphs, schema) =
                if let Ok(p) = uri.to_file_path() {
                    let project = config.get_project_for_path(&p);
                    (
                        project.and_then(|proj| proj.import().map(|s| s.to_string())),
                        package_roots.get(uri).and_then(|r| r.value().clone()),
                        project
                            .and_then(|proj| subgraphs.get(&proj.schema().as_key()))
                            .map(|r| r.value().clone()),
                        project
                            .and_then(|proj| schemas.get(&proj.schema().as_key()))
                            .map(|r| r.value().clone()),
                    )
                } else {
                    (None, None, None, None)
                };

            let doc = documents.get(uri).map(|r| r.value().clone());
            let mut fragment_nodes = AHashMap::default();

            if let Some(doc) = &doc {
                for block in doc.get_graphql_trees() {
                    let query = graphox_core::queries::GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
                        let lang = tree_sitter_graphql::LANGUAGE.into();
                        tree_sitter::Query::new(&lang, graphox_core::queries::GQL_SYMBOL_QUERY)
                            .unwrap()
                    });

                    let mut cursor = tree_sitter::QueryCursor::new();
                    let mut matches =
                        cursor.matches(query, block.tree.root_node(), |node: tree_sitter::Node| {
                            doc.rope
                                .byte_slice(
                                    (node.start_byte() + block.offset)
                                        ..(node.end_byte() + block.offset),
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
            }

            let documents = documents.clone();
            let uri_clone = uri.clone();
            let doc_clone = doc.clone();

            frags
                .iter()
                .map(move |frag| {
                    let mut worst_slo: Option<SloClass> = None;
                    if let (Some(schema), Some(subgraphs_list), Some((node, offset))) = (
                        &schema,
                        &project_subgraphs,
                        fragment_nodes.get(frag.name.as_ref()),
                    ) && let Some(selection) = doc_clone
                        .as_ref()
                        .and_then(|d| d.find_child_by_kind(*node, "selection_set"))
                        && let Some(type_cond) = schema.types.get(frag.type_condition.as_ref())
                    {
                        use graphox_features::hover::DocumentHover;
                        let mut visited = ahash::AHashSet::default();
                        visited.insert(frag.name.clone());
                        if let Some(doc) = documents.get(&uri_clone).map(|r| r.value().clone()) {
                            let slo = doc.calculate_worst_slo_for_selection_set(
                                selection,
                                *offset,
                                type_cond,
                                schema,
                                subgraphs_list,
                                &documents,
                                &mut visited,
                            );
                            if let Some(s) = slo {
                                worst_slo = Some(worst_slo.map_or(s, |w| w.worst(s)));
                            }
                        }
                    }

                    FragmentCompletionInfo {
                        name: frag.name.clone(),
                        type_condition: frag.type_condition.clone(),
                        description: frag.description.clone(),
                        import_path: import_path.as_deref().map(|s: &str| Arc::from(s)),
                        is_public: frag.is_public,
                        is_type_only: frag.is_type_only,
                        uri: uri_clone.clone(),
                        package_root: package_root.clone(),
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

pub fn collect_fragment_metadata_with_schema(
    fragment_defs: &Arc<DashMap<Url, Vec<FragmentDef>, ahash::RandomState>>,
    config: &Config,
    package_roots: &Arc<DashMap<Url, Option<PathBuf>, ahash::RandomState>>,
    subgraphs: &Arc<DashMap<String, Vec<SubgraphInfo>, ahash::RandomState>>,
    documents: &Arc<DashMap<Url, Arc<DocumentState>, ahash::RandomState>>,
    schemas: &Arc<DashMap<String, Arc<apollo_compiler::Schema>, ahash::RandomState>>,
) -> Vec<(FragmentCompletionInfo, Option<Arc<str>>)> {
    // Clone Arc references to avoid holding locks during iteration
    let fragment_defs = fragment_defs.clone();
    let package_roots = package_roots.clone();
    let subgraphs = subgraphs.clone();
    let documents = documents.clone();
    let schemas = schemas.clone();

    fragment_defs
        .iter()
        .flat_map(|entry| {
            let uri = entry.key();
            let frags = entry.value();

            let (import_path, schema_key, project_subgraphs, schema) =
                if let Ok(p) = uri.to_file_path() {
                    let project = config.get_project_for_path(&p);
                    let key = project.map(|proj| proj.schema().as_key());
                    (
                        project.and_then(|proj| proj.import().map(Arc::from)),
                        key.as_ref().map(|k| Arc::from(k.as_str())),
                        key.as_ref()
                            .and_then(|k| subgraphs.get(k))
                            .map(|r| r.value().clone()),
                        key.as_ref()
                            .and_then(|k| schemas.get(k))
                            .map(|r| r.value().clone()),
                    )
                } else {
                    (None, None, None, None)
                };

            let doc = documents.get(uri).map(|r| r.value().clone());
            let mut fragment_nodes = AHashMap::default();

            if let Some(doc) = &doc {
                for block in doc.get_graphql_trees() {
                    let query = graphox_core::queries::GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
                        let lang = tree_sitter_graphql::LANGUAGE.into();
                        tree_sitter::Query::new(&lang, graphox_core::queries::GQL_SYMBOL_QUERY)
                            .unwrap()
                    });

                    let mut cursor = tree_sitter::QueryCursor::new();
                    let mut matches =
                        cursor.matches(query, block.tree.root_node(), |node: tree_sitter::Node| {
                            doc.rope
                                .byte_slice(
                                    (node.start_byte() + block.offset)
                                        ..(node.end_byte() + block.offset),
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
            }

            let package_root = package_roots.get(uri).and_then(|r| r.value().clone());
            let documents = documents.clone();
            let uri_clone = uri.clone();
            let doc_clone = doc.clone();

            frags
                .iter()
                .map(move |frag| {
                    let mut worst_slo: Option<SloClass> = None;
                    if let (Some(schema), Some(subgraphs_list), Some((node, offset))) = (
                        &schema,
                        &project_subgraphs,
                        fragment_nodes.get(frag.name.as_ref()),
                    ) && let Some(selection) = doc_clone
                        .as_ref()
                        .and_then(|d| d.find_child_by_kind(*node, "selection_set"))
                        && let Some(type_cond) = schema.types.get(frag.type_condition.as_ref())
                    {
                        use graphox_features::hover::DocumentHover;
                        let mut visited = ahash::AHashSet::default();
                        visited.insert(frag.name.clone());
                        if let Some(doc) = documents.get(&uri_clone).map(|r| r.value().clone()) {
                            let slo = doc.calculate_worst_slo_for_selection_set(
                                selection,
                                *offset,
                                type_cond,
                                schema,
                                subgraphs_list,
                                &documents,
                                &mut visited,
                            );
                            if let Some(s) = slo {
                                worst_slo = Some(worst_slo.map_or(s, |w| w.worst(s)));
                            }
                        }
                    }

                    (
                        FragmentCompletionInfo {
                            name: frag.name.clone(),
                            type_condition: frag.type_condition.clone(),
                            description: frag.description.clone(),
                            import_path: import_path.clone(),
                            is_public: frag.is_public,
                            is_type_only: frag.is_type_only,
                            uri: uri_clone.clone(),
                            package_root: package_root.clone(),
                            used_variables: frag.used_variables.clone(),
                            used_fragments: frag.used_fragments.clone(),
                            transitive_deps: frag.transitive_deps.clone(),
                            selected_fields: frag.selected_fields.clone(),
                            type_fields: frag.type_fields.clone(),
                            requirements: std::collections::BTreeMap::new(),
                            worst_slo,
                        },
                        schema_key.clone(),
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
    old_spreads: Option<Vec<Arc<str>>>,
    new_spreads: Vec<Arc<str>>,
) {
    if let Some(old) = old_spreads {
        for spread in old {
            if !new_spreads.contains(&spread)
                && let Some(mut entry) = fragment_dependents.get_mut(&spread)
            {
                entry.value_mut().remove(uri);
            }
        }
    }

    for spread in new_spreads {
        fragment_dependents
            .entry(spread)
            .or_default()
            .insert(uri.clone());
    }
}

/// Updates the fragment definition index when fragments are added/removed
pub fn update_fragment_definitions(
    fragment_definitions: &Arc<DashMap<Arc<str>, AHashSet<Url>, ahash::RandomState>>,
    uri: &Url,
    old_fragments: Option<Vec<Arc<str>>>,
    new_fragments: Vec<Arc<str>>,
) {
    if let Some(old) = old_fragments {
        for name in old {
            if !new_fragments.contains(&name)
                && let Some(mut entry) = fragment_definitions.get_mut(&name)
            {
                entry.value_mut().remove(uri);
            }
        }
    }

    for name in new_fragments {
        fragment_definitions
            .entry(name)
            .or_default()
            .insert(uri.clone());
    }
}
