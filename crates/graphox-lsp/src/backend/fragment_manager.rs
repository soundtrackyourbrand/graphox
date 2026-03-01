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
pub fn collect_fragment_metadata(
    metadata: &MetadataMap,
    config: &Config,
    subgraphs: &Arc<DashMap<String, Vec<SubgraphInfo>, ahash::RandomState>>,
    documents: &Arc<DashMap<Url, Arc<DocumentState>, ahash::RandomState>>,
    schemas: &Arc<DashMap<String, Arc<apollo_compiler::Schema>, ahash::RandomState>>,
) -> Vec<FragmentCompletionInfo> {
    // Clone Arc references to avoid holding locks during iteration
    let metadata_arc = metadata.clone();
    let subgraphs_arc = subgraphs.clone();
    let documents_arc = documents.clone();
    let schemas_arc = schemas.clone();

    metadata_arc
        .iter()
        .flat_map(|entry| {
            let uri = entry.key();
            let meta = entry.value();

            // Get project info once per file
            let (import_path, project_subgraphs, schema) = if let Ok(p) = uri.to_file_path() {
                let project = config.get_project_for_path(&p);
                let schema_key = project.map(|p| p.schema().as_key());
                (
                    project.and_then(|proj| proj.import().map(|s| s.to_string())),
                    schema_key
                        .as_ref()
                        .and_then(|key: &String| subgraphs_arc.get(key.as_str()))
                        .map(|r| r.value().clone()),
                    schema_key
                        .as_ref()
                        .and_then(|key: &String| schemas_arc.get(key.as_str()))
                        .map(|r| r.value().clone()),
                )
            } else {
                (None, None, None)
            };

            let doc = documents_arc.get(uri).map(|r| r.value().clone());
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

            let documents_clone = documents_arc.clone();
            let uri_clone = uri.clone();
            let doc_clone = doc.clone();
            let project_subgraphs_clone: Option<Vec<SubgraphInfo>> = project_subgraphs;
            let schema_clone: Option<Arc<apollo_compiler::Schema>> = schema;

            meta.fragments
                .iter()
                .map(move |frag| {
                    let mut worst_slo: Option<SloClass> = None;

                    if let Some(schema_val) = schema_clone.as_ref()
                        && let Some(subgraphs_list) = project_subgraphs_clone.as_ref()
                        && let Some((node, offset)) = fragment_nodes.get(frag.name.as_ref())
                        && let Some(selection) = doc_clone
                            .as_ref()
                            .and_then(|d| d.find_child_by_kind(*node, "selection_set"))
                        && let Some(type_cond) = schema_val.types.get(frag.type_condition.as_ref())
                    {
                        use graphox_features::hover::DocumentHover;
                        let mut visited = ahash::AHashSet::default();
                        visited.insert(frag.name.clone());
                        if let Some(doc) =
                            documents_clone.get(&uri_clone).map(|r| r.value().clone())
                        {
                            let slo = doc.calculate_worst_slo_for_selection_set(
                                selection,
                                *offset,
                                type_cond,
                                schema_val,
                                subgraphs_list,
                                &documents_clone,
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

pub fn collect_fragment_metadata_with_schema(
    metadata: &MetadataMap,
    config: &Config,
    subgraphs: &Arc<DashMap<String, Vec<SubgraphInfo>, ahash::RandomState>>,
    documents: &Arc<DashMap<Url, Arc<DocumentState>, ahash::RandomState>>,
    schemas: &Arc<DashMap<String, Arc<apollo_compiler::Schema>, ahash::RandomState>>,
) -> Vec<(FragmentCompletionInfo, Option<Arc<str>>)> {
    // Clone Arc references to avoid holding locks during iteration
    let metadata_arc = metadata.clone();
    let subgraphs_arc = subgraphs.clone();
    let documents_arc = documents.clone();
    let schemas_arc = schemas.clone();

    metadata_arc
        .iter()
        .flat_map(|entry| {
            let uri = entry.key();
            let meta = entry.value();

            let (import_path, schema_key, project_subgraphs, schema) =
                if let Ok(p) = uri.to_file_path() {
                    let project = config.get_project_for_path(&p);
                    let key = project.map(|proj| Arc::from(proj.schema().as_key()));
                    (
                        project.and_then(|proj| proj.import().map(Arc::from)),
                        key.clone(),
                        key.as_ref()
                            .and_then(|k: &Arc<str>| subgraphs_arc.get::<str>(k.as_ref()))
                            .map(|r| r.value().clone()),
                        key.as_ref()
                            .and_then(|k: &Arc<str>| schemas_arc.get::<str>(k.as_ref()))
                            .map(|r| r.value().clone()),
                    )
                } else {
                    (None, None, None, None)
                };

            let doc = documents_arc.get(uri).map(|r| r.value().clone());
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

            let documents_clone = documents_arc.clone();
            let uri_clone = uri.clone();
            let doc_clone = doc.clone();
            let project_subgraphs_clone: Option<Vec<SubgraphInfo>> = project_subgraphs;
            let schema_clone: Option<Arc<apollo_compiler::Schema>> = schema;
            let schema_key_clone: Option<Arc<str>> = schema_key;

            meta.fragments
                .iter()
                .map(move |frag| {
                    let mut worst_slo: Option<SloClass> = None;

                    if let Some(schema_val) = schema_clone.as_ref()
                        && let Some(subgraphs_list) = project_subgraphs_clone.as_ref()
                        && let Some((node, offset)) = fragment_nodes.get(frag.name.as_ref())
                        && let Some(selection) = doc_clone
                            .as_ref()
                            .and_then(|d| d.find_child_by_kind(*node, "selection_set"))
                        && let Some(type_cond) = schema_val.types.get(frag.type_condition.as_ref())
                    {
                        use graphox_features::hover::DocumentHover;
                        let mut visited = ahash::AHashSet::default();
                        visited.insert(frag.name.clone());
                        if let Some(doc) =
                            documents_clone.get(&uri_clone).map(|r| r.value().clone())
                        {
                            let slo = doc.calculate_worst_slo_for_selection_set(
                                selection,
                                *offset,
                                type_cond,
                                schema_val,
                                subgraphs_list,
                                &documents_clone,
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
