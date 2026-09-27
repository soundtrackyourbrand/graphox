//! Changes made outside the editor, and how the server applies the
//! watched-file events that report them.

use crate::support::{self, LspBackend};
use graphox::Backend;
use graphox::Config;
use graphox::config::{CodegenConfig, GlobPattern as Include, ProjectConfig, SchemaSource};
use graphox_lsp::backend::handlers::document_sync::process_watched_file_batch;
use std::fs;
use std::path::Path;
use std::sync::atomic::Ordering;
use tower_lsp_server::LspService;
use tower_lsp_server::ls_types::*;

const SCHEMA: &str = "type Query { user: User } type User { id: ID! name: String }";

async fn scanned_service(base: &Path) -> LspService<LspBackend> {
    let config = Config::new_test(
        base.to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(Include::Multiple(vec![
                    "src/**/*.graphql".to_string(),
                    "src/**/*.ts".to_string(),
                ]))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_lsp_automatic_codegen(false);
    let (mut service, _handle) = support::create_service(config);
    support::lsp_initialize_sequence(&mut service).await;
    let backend = service.inner();
    assert!(
        support::wait_for_condition(|| backend.workspace_loaded.load(Ordering::SeqCst)).await,
        "workspace scan did not finish"
    );
    service
}

fn workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    fs::create_dir_all(base.join(".git")).unwrap();
    fs::write(base.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    fs::write(base.join("schema.graphql"), SCHEMA).unwrap();
    for (path, contents) in files {
        let path = base.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    dir
}

/// Whether any document defines `name`. The index keeps a name's entry, empty,
/// after its last definition goes.
fn is_defined(backend: &Backend, name: &str) -> bool {
    backend
        .fragment_definitions
        .get(name)
        .is_some_and(|uris| !uris.is_empty())
}

fn changed(base: &Path, rel: &str) -> FileEvent {
    FileEvent {
        uri: graphox::utils::path_to_uri(base.join(rel)).unwrap(),
        typ: FileChangeType::CHANGED,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_nested_ignore_all_rule_does_not_hide_the_rest_of_the_workspace() {
    let dir = workspace(&[
        ("src/fragment.graphql", "fragment Before on User { id }"),
        ("src/generated/.gitignore", "/*\n!.gitignore\n"),
    ]);
    let base = dir.path().canonicalize().unwrap();
    let service = scanned_service(&base).await;
    let backend = service.inner();
    assert!(is_defined(backend, "Before"));

    fs::write(
        base.join("src/fragment.graphql"),
        "fragment After on User { id }",
    )
    .unwrap();
    process_watched_file_batch(backend, vec![changed(&base, "src/fragment.graphql")]).await;

    assert!(is_defined(backend, "After"));
    assert!(!is_defined(backend, "Before"));
}
