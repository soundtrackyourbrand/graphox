//! Workspace scanning and indexing operations
//!
//! This module handles the background workspace scan that occurs when the LSP
//! initializes. It parses all GraphQL files, indexes fragments, and validates
//! documents in parallel.

use crate::config::Config;
use crate::document::DocumentState;
use crate::features::completion::FragmentCompletionInfo;
use apollo_compiler::Schema;
use dashmap::DashMap;
use fnv::FnvHashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tower_lsp::lsp_types::*;
use tower_lsp::Client;
use tree_sitter::StreamingIterator;

/// Parameters for workspace scanning operation
pub struct WorkspaceScanParams {
    pub client: Client,
    pub config: Config,
    pub documents: Arc<DashMap<Url, Arc<DocumentState>, ahash::RandomState>>,
    pub fragment_defs: Arc<DashMap<Url, Vec<crate::document::FragmentDef>, ahash::RandomState>>,
    pub fragment_spreads: Arc<DashMap<Url, Vec<String>, ahash::RandomState>>,
    pub package_roots: Arc<DashMap<Url, Option<PathBuf>, ahash::RandomState>>,
    pub fragment_dependents: Arc<DashMap<String, FnvHashSet<Url>, ahash::RandomState>>,
    pub fragment_definitions: Arc<DashMap<String, FnvHashSet<Url>, ahash::RandomState>>,
    pub workspace_loaded: Arc<AtomicBool>,
    pub empty_schema: Arc<Schema>,
    pub schemas: Arc<DashMap<String, Arc<Schema>, ahash::RandomState>>,
    pub workspace_scan_cancelled: Arc<AtomicBool>,
}

/// Spawns a background workspace scan task
///
/// This function extracts the large workspace scanning logic from Backend::initialized().
/// It runs in a separate tokio task to avoid blocking the LSP during initialization.
pub fn spawn_workspace_scan(params: WorkspaceScanParams) {
    tokio::spawn(async move {
        let token = NumberOrString::String("workspace-scan".to_string());
        let cancelled = params.workspace_scan_cancelled;

        // Create progress in a separate task so it doesn't block the scan
        let client_clone = params.client.clone();
        let token_clone = token.clone();
        tokio::spawn(async move {
            let _ = client_clone
                .send_request::<request::WorkDoneProgressCreate>(WorkDoneProgressCreateParams {
                    token: token_clone.clone(),
                })
                .await;

            // Begin progress
            let _ = client_clone
                .send_notification::<notification::Progress>(ProgressParams {
                    token: token_clone,
                    value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(
                        WorkDoneProgressBegin {
                            title: "Scanning workspace".to_string(),
                            cancellable: Some(true),
                            message: Some("Parsing GraphQL files...".to_string()),
                            percentage: Some(0),
                        },
                    )),
                })
                .await;
        });

        // Scan workspace and index all fragments/spreads
        let workspace_metadata = scan_and_index_workspace(
            &params.config,
            &params.fragment_defs,
            &params.fragment_spreads,
            &params.package_roots,
            &params.fragment_dependents,
            &params.fragment_definitions,
            &params.documents,
            &params.client,
            &token,
            &cancelled,
        );

        let total_docs = workspace_metadata.documents.len();
        params.workspace_loaded.store(true, Ordering::SeqCst);

        // Validate all documents with proper schemas and fragments
        validate_all_documents(
            &params.documents,
            &params.config,
            &params.fragment_defs,
            &params.fragment_spreads,
            &params.package_roots,
            &params.schemas,
            &params.empty_schema,
            &params.client,
        )
        .await;

        // End progress
        let _ = params.client
            .send_notification::<notification::Progress>(ProgressParams {
                token,
                value: ProgressParamsValue::WorkDone(WorkDoneProgress::End(
                    WorkDoneProgressEnd {
                        message: Some(format!("Finished scanning {} files", total_docs)),
                    },
                )),
            })
            .await;

        params.client
            .log_message(MessageType::INFO, "Workspace scan complete.")
            .await;
    });
}

/// Scans workspace and indexes all fragments and spreads
fn scan_and_index_workspace(
    config: &Config,
    fragment_defs: &Arc<DashMap<Url, Vec<crate::document::FragmentDef>, ahash::RandomState>>,
    fragment_spreads: &Arc<DashMap<Url, Vec<String>, ahash::RandomState>>,
    package_roots: &Arc<DashMap<Url, Option<PathBuf>, ahash::RandomState>>,
    fragment_dependents: &Arc<DashMap<String, FnvHashSet<Url>, ahash::RandomState>>,
    fragment_definitions: &Arc<DashMap<String, FnvHashSet<Url>, ahash::RandomState>>,
    documents: &Arc<DashMap<Url, Arc<DocumentState>, ahash::RandomState>>,
    client: &Client,
    token: &NumberOrString,
    cancelled: &Arc<AtomicBool>,
) -> crate::engine::WorkspaceMetadata {
    crate::engine::Engine::scan_workspace_cancellable(
        config,
        |_, doc| {
            if cancelled.load(Ordering::Relaxed) {
                return;
            }
            let uri = doc.uri.clone();

            fragment_defs.insert(uri.clone(), doc.fragments().to_vec());
            fragment_spreads.insert(uri.clone(), doc.fragment_spreads.clone());
            package_roots.insert(uri.clone(), doc.package_root.clone());

            for frag in doc.fragments() {
                fragment_definitions
                    .entry(frag.name.clone())
                    .or_default()
                    .insert(uri.clone());
            }

            // Also index type definitions (for Go to Definition)
            let query = crate::queries::GQL_DEFINITION_QUERY_CACHE.get_or_init(|| {
                let lang = tree_sitter_graphql::LANGUAGE.into();
                tree_sitter::Query::new(&lang, crate::queries::GQL_DEFINITION_QUERY).unwrap()
            });
            let mut cursor = tree_sitter::QueryCursor::new();
            for block in doc.get_graphql_trees() {
                let mut matches = cursor.matches(
                    query,
                    block.tree.root_node(),
                    |node: tree_sitter::Node| {
                        doc.rope
                            .byte_slice(
                                (node.start_byte() + block.offset)
                                    ..(node.end_byte() + block.offset),
                            )
                            .chunks()
                    },
                );
                while let Some(m) = matches.next() {
                    let name_node = m.captures[0].node;
                    let name = doc.get_node_text(name_node, block.offset);
                    fragment_definitions
                        .entry(name)
                        .or_default()
                        .insert(uri.clone());
                }
            }

            for spread in &doc.fragment_spreads {
                fragment_dependents
                    .entry(spread.clone())
                    .or_default()
                    .insert(uri.clone());
            }

            // If the document is not already open, we still might want to keep it in memory
            // for fast definition/hover/etc.
            if !documents.contains_key(&uri) {
                documents.insert(uri, Arc::new(doc));
            }
        },
        |current, total| {
            if total == 0 {
                return;
            }
            let percentage = (current * 100 / total) as u32;
            let client = client.clone();
            let token = token.clone();
            tokio::spawn(async move {
                let _ = client
                    .send_notification::<notification::Progress>(ProgressParams {
                        token,
                        value: ProgressParamsValue::WorkDone(WorkDoneProgress::Report(
                            WorkDoneProgressReport {
                                cancellable: Some(true),
                                message: Some(format!(
                                    "Parsing GraphQL files... ({}/{})",
                                    current, total
                                )),
                                percentage: Some(percentage),
                            },
                        )),
                    })
                    .await;
            });
        },
        cancelled.clone(),
    )
}

/// Validates all documents in the workspace
async fn validate_all_documents(
    documents: &Arc<DashMap<Url, Arc<DocumentState>, ahash::RandomState>>,
    config: &Config,
    fragment_defs: &Arc<DashMap<Url, Vec<crate::document::FragmentDef>, ahash::RandomState>>,
    fragment_spreads: &Arc<DashMap<Url, Vec<String>, ahash::RandomState>>,
    package_roots: &Arc<DashMap<Url, Option<PathBuf>, ahash::RandomState>>,
    schemas: &Arc<DashMap<String, Arc<Schema>, ahash::RandomState>>,
    empty_schema: &Arc<Schema>,
    client: &Client,
) {
    // Collect all used fragments
    let used_fragments = {
        let mut used = FnvHashSet::default();
        for entry in fragment_spreads.iter() {
            for spread in entry.value() {
                used.insert(spread.clone());
            }
        }
        used
    };

    // Pre-calculate validated schemas to avoid repeated validation
    let mut validated_schemas_map = fnv::FnvHashMap::default();
    for entry in schemas.iter() {
        if let Ok(valid) = (**entry.value()).clone().validate() {
            validated_schemas_map.insert(entry.key().clone(), Arc::new(valid));
        }
    }
    let valid_empty_schema = Arc::new(
        <apollo_compiler::Schema as Clone>::clone(empty_schema)
            .validate()
            .unwrap(),
    );

    // Pre-calculate all fragments info
    let all_fragments_info: Vec<(FragmentCompletionInfo, Option<String>)> =
        super::fragment_manager::collect_fragment_metadata_with_schema(
            fragment_defs,
            config,
            package_roots,
        );

    // Validate all documents in parallel
    use rayon::prelude::*;
    let to_publish: Vec<(Url, Vec<Diagnostic>)> = documents
        .as_ref()
        .par_iter()
        .map(|entry| {
            let uri = entry.key();
            let doc = entry.value();

            // Get schema for doc
            let (schema_key, schema): (
                Option<String>,
                Arc<apollo_compiler::validation::Valid<Schema>>,
            ) = if let Ok(path) = uri.to_file_path()
                && let Some(schema_path) = config.get_schema_for_path(&path)
                && let Some(schema) = validated_schemas_map.get(&schema_path)
            {
                (Some(schema_path), schema.clone())
            } else {
                (None, valid_empty_schema.clone())
            };

            // Filter fragments for this doc (same project/schema or public)
            let target_package_root = doc.package_root.as_ref();
            let filtered_fragments: Vec<FragmentCompletionInfo> = all_fragments_info
                .iter()
                .filter(|(f, f_schema_key)| {
                    let is_same_project = f_schema_key.is_some() && f_schema_key == &schema_key;
                    let is_same_package = f.package_root.as_ref() == target_package_root;
                    is_same_project || is_same_package || f.is_public
                })
                .map(|(f, _)| f.clone())
                .collect();

            // If there are duplicate fragment names, prioritize the one in the same package,
            // then same project, then public.
            let mut sorted_fragments = filtered_fragments;
            sorted_fragments.sort_by(|a, b| {
                let a_same_pkg = a.package_root.as_ref() == target_package_root;
                let b_same_pkg = b.package_root.as_ref() == target_package_root;

                if a_same_pkg != b_same_pkg {
                    return b_same_pkg.cmp(&a_same_pkg);
                }

                // If both (or neither) are same package, prefer same project (already filtered)
                b.is_public.cmp(&a.is_public).reverse() // prefer non-public (local)
            });

            let diagnostics = doc.get_semantic_diagnostics(
                &schema,
                &sorted_fragments,
                Some(&used_fragments),
                Some(config),
                false,
                true,
            );
            (uri.clone(), diagnostics)
        })
        .collect();

    for (u, d) in to_publish {
        client.publish_diagnostics(u, d, None).await;
    }
}
