use criterion::{Criterion, criterion_group, criterion_main};
use dashmap::DashMap;
use graphox_core::Config;
use graphox_core::config::{GlobPattern, ProjectConfig, SchemaSource};
use graphox_core::document::OperationDef;
use graphox_core::types::OperationNamesMap;
use graphox_features::diagnostics::DocumentDiagnostics;
use graphox_lsp::backend::helpers::update_operation_name_index;
use graphox_lsp::backend::state::Backend;
use graphox_lsp::backend::validation::ValidationParams;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tempfile::tempdir;
use tower_lsp::LanguageServer;
use tower_lsp::lsp_types::*;

pub fn bench_lsp_actions(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();

    let dir = tempdir().unwrap();
    let base_dir = dir.path().canonicalize().unwrap();
    let config = Config::new_test(
        base_dir.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("**/*.graphql".to_string())),
        ],
    );

    // Create schema file
    std::fs::write(
        base_dir.join("schema.graphql"),
        "type Query { user: User } type User { id: ID! name: String }",
    )
    .unwrap();

    let _guard = rt.enter();
    let (service, _) = tower_lsp::LspService::new(|client| Backend::new(client, config.clone()));
    let backend = service.inner();

    // Seed some documents
    let num_docs = 50;
    for i in 0..num_docs {
        let uri = Url::from_file_path(base_dir.join(format!("doc_{}.graphql", i))).unwrap();
        let content = format!("query GetUser{} {{ user {{ id name }} }}", i);
        let doc = graphox_core::DocumentState::new_from_thread_local(
            uri.clone(),
            &content,
            PositionEncodingKind::UTF16,
        );
        backend.documents.insert(uri.clone(), Arc::new(doc));

        let metadata = Arc::new(graphox_core::types::DocumentMetadata {
            fragments: Arc::from([]),
            fragment_spreads: Arc::from([]),
            package_root: None,
            operations: Arc::from([]),
            version: 0,
        });
        backend.metadata.insert(uri, metadata);
    }

    let target_uri = Url::from_file_path(base_dir.join("doc_0.graphql")).unwrap();
    let index_uri = Url::from_file_path(base_dir.join("ops.graphql")).unwrap();
    let other_uri = Url::from_file_path(base_dir.join("ops_other.graphql")).unwrap();
    std::fs::write(
        base_dir.join("ops.graphql"),
        "query SharedQuery { user { id } }",
    )
    .unwrap();
    std::fs::write(
        base_dir.join("ops_other.graphql"),
        "query SharedQuery { user { id } }",
    )
    .unwrap();
    let operation_index: OperationNamesMap =
        Arc::new(DashMap::with_hasher(ahash::RandomState::default()));
    operation_index.insert(
        Arc::from("SharedQuery"),
        vec![
            (Arc::from("**/*.graphql"), index_uri.clone()),
            (Arc::from("**/*.graphql"), other_uri.clone()),
        ],
    );
    operation_index.insert(
        Arc::from("SecondaryQuery"),
        vec![(Arc::from("**/*.graphql"), index_uri.clone())],
    );
    let old_operation_names: Arc<[Arc<str>]> =
        vec![Arc::from("SharedQuery"), Arc::from("SecondaryQuery")].into();
    let replacement_operations = vec![
        OperationDef {
            name: Some(Arc::from("SharedQuery")),
            operation_type: Arc::from("query"),
            source_text: Arc::from("query SharedQuery { user { id } }"),
        },
        OperationDef {
            name: Some(Arc::from("SecondaryQuery")),
            operation_type: Arc::from("query"),
            source_text: Arc::from("query SecondaryQuery { user { id } }"),
        },
    ];

    let mut group = c.benchmark_group("LSP Actions");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(100));

    group.bench_function("Run Codegen", |b| {
        b.to_async(&rt).iter(|| backend.run_codegen());
    });

    group.bench_function("Check Workspace (Full Diagnostics)", |b| {
        b.to_async(&rt).iter(|| async {
            let config_val = backend.config.read().unwrap().clone();
            let all_uris: Vec<Url> = backend.documents.iter().map(|e| e.key().clone()).collect();
            let validated_schemas = backend.validated_schemas.clone();
            let valid_empty_schema = backend.valid_empty_schema.clone();
            let workspace_loaded = backend.workspace_loaded.clone();
            let open_documents = backend.open_documents.clone();
            let fragment_dependents = backend.fragment_dependents.clone();
            let fragment_definitions = backend.fragment_definitions.clone();
            let operation_names = backend.operation_names.clone();

            let params = ValidationParams {
                client: &backend.client,
                documents: &backend.documents,
                metadata: &backend.metadata,
                config: &config_val,
                validated_schemas: &validated_schemas,
                valid_empty_schema: &valid_empty_schema,
                workspace_loaded: &workspace_loaded,
                open_documents: &open_documents,
                fragment_dependents: &fragment_dependents,
                fragment_definitions: &fragment_definitions,
                operation_names: &operation_names,
                subgraphs: &backend.subgraphs,
                schemas: &backend.schemas,
                supports_progress: false,
                position_encoding: PositionEncodingKind::UTF16,
                result_id_epoch: backend.last_full_validation_version.load(Ordering::SeqCst),
                validation_fragment_cache: Some(&backend.validation_fragment_cache),
            };

            graphox_lsp::backend::validation::validate_uris(params, all_uris, false, None).await;
        });
    });

    // Single document diagnostics (matches user's textDocument/diagnostic call)
    group.bench_function("Document Diagnostic", |b| {
        b.to_async(&rt).iter(|| async {
            let doc = backend
                .documents
                .get(&target_uri)
                .map(|r| r.value().clone())
                .unwrap();
            let schema = backend.get_schema_for_doc(&target_uri);
            let all_fragments = backend.get_all_fragments_info();
            let fragments = backend.get_fragments_for_doc(&doc, &all_fragments);
            let used_fragments = backend.get_used_fragments();

            doc.get_semantic_diagnostics(
                &schema,
                &fragments,
                Some(&used_fragments),
                None,
                false,
                true,
            );
        })
    });

    group.bench_function("Document Highlight", |b| {
        b.to_async(&rt).iter(|| {
            backend.document_highlight(DocumentHighlightParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: target_uri.clone(),
                    },
                    position: Position::new(0, 10),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
        });
    });

    group.bench_function("Update Operation Name Index", |b| {
        b.iter(|| {
            let affected = update_operation_name_index(
                &operation_index,
                &config,
                &index_uri,
                Some(old_operation_names.as_ref()),
                &replacement_operations,
            );
            std::hint::black_box(affected);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_lsp_actions);
criterion_main!(benches);
