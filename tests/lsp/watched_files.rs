//! Changes made outside the editor: the file watchers the server registers,
//! and how it applies the watched-file events that come back.

use crate::support::{self, LspBackend};
use futures_util::{SinkExt, StreamExt};
use graphox::Backend;
use graphox::Config;
use graphox::config::{CodegenConfig, GlobPattern as Include, ProjectConfig, SchemaSource};
use graphox_lsp::backend::capabilities::ClientCapabilities as GraphoxClientCapabilities;
use graphox_lsp::backend::file_watchers::watcher_registration;
use graphox_lsp::backend::handlers::document_sync::process_watched_file_batch;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::sync::atomic::Ordering;
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};
use tower_lsp_server::LspService;
use tower_lsp_server::jsonrpc::{Request, Response};
use tower_lsp_server::ls_types::*;
use tower_service::Service;

const ALL_FILES_GLOB: &str = "**/*.{graphql,gql,ts,tsx,mts,cts,js,jsx,mjs,cjs}";

/// A service whose client answers every server request with `null`, as an
/// editor does, and reports each request's method and params.
fn answering_service(config: Config) -> (LspService<LspBackend>, mpsc::UnboundedReceiver<Value>) {
    answering_service_holding_first_registration(config, None)
}

/// Like [`answering_service`], but the answer to the first
/// `client/registerCapability` waits for `release`, as a slow client's would.
fn answering_service_holding_first_registration(
    config: Config,
    release: Option<tokio::sync::oneshot::Receiver<()>>,
) -> (LspService<LspBackend>, mpsc::UnboundedReceiver<Value>) {
    let (service, socket) =
        LspService::new(|client| graphox::GraphoxLanguageServer::new(Backend::new(client, config)));
    let (mut requests, mut responses) = socket.split();
    let (tx, rx) = mpsc::unbounded_channel();
    let (answer_tx, mut answers) = mpsc::unbounded_channel::<Response>();
    tokio::spawn(async move {
        while let Some(answer) = answers.recv().await {
            let _ = responses.send(answer).await;
        }
    });
    tokio::spawn(async move {
        let mut release = release;
        while let Some(request) = requests.next().await {
            let _ = tx.send(json!({
                "method": request.method(),
                "params": request.params().cloned(),
            }));
            let Some(id) = request.id().cloned() else {
                continue;
            };
            let answer = Response::from_ok(id, Value::Null);
            match release.take() {
                Some(release) if request.method() == "client/registerCapability" => {
                    let answer_tx = answer_tx.clone();
                    tokio::spawn(async move {
                        let _ = release.await;
                        let _ = answer_tx.send(answer);
                    });
                }
                other => {
                    release = other;
                    let _ = answer_tx.send(answer);
                }
            }
        }
    });
    (service, rx)
}

async fn initialize(service: &mut LspService<LspBackend>, capabilities: Value) {
    service
        .call(
            Request::build("initialize")
                .params(json!({ "capabilities": capabilities }))
                .id(1)
                .finish(),
        )
        .await
        .unwrap();
    service
        .call(Request::build("initialized").params(json!({})).finish())
        .await
        .unwrap();
}

fn watching_client(relative_patterns: bool) -> Value {
    json!({
        "workspace": {
            "didChangeWatchedFiles": {
                "dynamicRegistration": true,
                "relativePatternSupport": relative_patterns
            }
        }
    })
}

/// Waits for the next server request with `method`, returning its params and
/// the methods of the requests that came before it.
async fn next_request(
    rx: &mut mpsc::UnboundedReceiver<Value>,
    method: &str,
) -> (Value, Vec<String>) {
    let mut before = Vec::new();
    timeout(Duration::from_secs(10), async {
        loop {
            let message = rx.recv().await.expect("server closed the connection");
            let this = message["method"].as_str().unwrap_or_default().to_string();
            if this == method {
                return (message["params"].clone(), before);
            }
            before.push(this);
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no {method} request from the server"))
}

fn registered_watchers(params: &Value) -> Vec<FileSystemWatcher> {
    let registration = &params["registrations"][0];
    assert_eq!(registration["method"], "workspace/didChangeWatchedFiles");
    let options: DidChangeWatchedFilesRegistrationOptions =
        serde_json::from_value(registration["registerOptions"].clone()).unwrap();
    options.watchers
}

fn watcher(glob_pattern: GlobPattern) -> FileSystemWatcher {
    FileSystemWatcher {
        glob_pattern,
        kind: Some(WatchKind::all()),
    }
}

fn write_workspace(base: &Path, schema: &str) {
    fs::create_dir_all(base.join(Path::new(schema).parent().unwrap())).unwrap();
    fs::write(base.join(schema), "type Query { id: ID }").unwrap();
    fs::write(
        base.join("graphox.yaml"),
        format!("projects:\n  - schema: {schema}\n    include: \"src/**/*.graphql\"\n"),
    )
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registers_config_schema_and_document_watchers() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    write_workspace(&base, "schema/schema.graphqls");
    let config = Config::load_from_dir(&base).unwrap().unwrap();

    let (mut service, mut rx) = answering_service(config);
    initialize(&mut service, watching_client(true)).await;
    let (params, _) = next_request(&mut rx, "client/registerCapability").await;

    let base_uri = graphox::utils::path_to_uri(&base).unwrap();
    assert_eq!(
        registered_watchers(&params),
        vec![
            watcher(GlobPattern::String(
                base.join("graphox.yaml").to_string_lossy().into_owned()
            )),
            // Watched even though it does not exist, so creating it is seen.
            watcher(GlobPattern::String(
                base.join("graphox.yml").to_string_lossy().into_owned()
            )),
            // A relative string glob would be matched against absolute paths
            // and never fire, so the schema is anchored at the workspace.
            watcher(GlobPattern::Relative(RelativePattern {
                base_uri: OneOf::Right(base_uri),
                pattern: "schema/schema.graphqls".to_string(),
            })),
            watcher(GlobPattern::String(ALL_FILES_GLOB.to_string())),
        ]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn schema_watchers_are_absolute_without_relative_pattern_support() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    write_workspace(&base, "schema/schema.graphqls");
    let config = Config::load_from_dir(&base).unwrap().unwrap();

    let (mut service, mut rx) = answering_service(config);
    initialize(&mut service, watching_client(false)).await;
    let (params, _) = next_request(&mut rx, "client/registerCapability").await;

    let schema = base.join("schema/schema.graphqls");
    assert!(
        registered_watchers(&params).contains(&watcher(GlobPattern::String(
            schema.to_string_lossy().into_owned()
        ))),
        "schema watched by absolute path"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_reload_replaces_the_registration() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    write_workspace(&base, "schema/schema.graphqls");
    let config = Config::load_from_dir(&base).unwrap().unwrap();

    let (mut service, mut rx) = answering_service(config);
    initialize(&mut service, watching_client(true)).await;
    next_request(&mut rx, "client/registerCapability").await;

    write_workspace(&base, "other/schema.graphqls");
    let config_uri = graphox::utils::path_to_uri(base.join("graphox.yaml")).unwrap();
    service
        .call(
            Request::build("workspace/didChangeWatchedFiles")
                .params(json!({ "changes": [{ "uri": config_uri, "type": 2 }] }))
                .finish(),
        )
        .await
        .unwrap();

    let (unregister, _) = next_request(&mut rx, "client/unregisterCapability").await;
    assert_eq!(unregister["unregisterations"][0]["id"], "watch-files");
    let (params, _) = next_request(&mut rx, "client/registerCapability").await;
    let patterns: Vec<_> = registered_watchers(&params)
        .into_iter()
        .filter_map(|w| match w.glob_pattern {
            GlobPattern::Relative(r) => Some(r.pattern),
            GlobPattern::String(_) => None,
        })
        .collect();
    assert_eq!(patterns, vec!["other/schema.graphqls".to_string()]);
}

async fn change_config(service: &mut LspService<LspBackend>, base: &Path, schema: &str) {
    write_workspace(base, schema);
    let config_uri = graphox::utils::path_to_uri(base.join("graphox.yaml")).unwrap();
    service
        .call(
            Request::build("workspace/didChangeWatchedFiles")
                .params(json!({ "changes": [{ "uri": config_uri, "type": 2 }] }))
                .finish(),
        )
        .await
        .unwrap();
}

fn schema_patterns(params: &Value) -> Vec<String> {
    registered_watchers(params)
        .into_iter()
        .filter_map(|w| match w.glob_pattern {
            GlobPattern::Relative(r) => Some(r.pattern),
            GlobPattern::String(_) => None,
        })
        .collect()
}

/// Two reloads while the client is still answering an earlier registration:
/// the registrations are applied in order, and the superseded one is skipped,
/// so the client ends up watching the newest config's schema.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reloads_while_a_registration_is_pending_leave_the_newest_registered() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    write_workspace(&base, "first/schema.graphqls");
    let config = Config::load_from_dir(&base).unwrap().unwrap();

    let (release, released) = tokio::sync::oneshot::channel();
    let (mut service, mut rx) =
        answering_service_holding_first_registration(config, Some(released));
    initialize(&mut service, watching_client(true)).await;
    let (first, _) = next_request(&mut rx, "client/registerCapability").await;
    assert_eq!(
        schema_patterns(&first),
        vec!["first/schema.graphqls".to_string()]
    );

    change_config(&mut service, &base, "second/schema.graphqls").await;
    change_config(&mut service, &base, "third/schema.graphqls").await;
    release.send(()).unwrap();

    let (unregister, _) = next_request(&mut rx, "client/unregisterCapability").await;
    assert_eq!(unregister["unregisterations"][0]["id"], "watch-files");
    let (params, before) = next_request(&mut rx, "client/registerCapability").await;
    assert!(
        !before.iter().any(|m| m == "client/registerCapability"),
        "only one registration after the release: {before:?}"
    );
    assert_eq!(
        schema_patterns(&params),
        vec!["third/schema.graphqls".to_string()]
    );
}

#[test]
fn registration_needs_the_client_to_allow_it() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    write_workspace(&base, "schema.graphql");
    let config = Config::load_from_dir(&base).unwrap().unwrap();

    assert!(watcher_registration(&config, &GraphoxClientCapabilities::default()).is_none());

    let capabilities = GraphoxClientCapabilities {
        supports_watched_files_registration: true,
        ..Default::default()
    };
    assert!(watcher_registration(&config, &capabilities).is_some());
}

#[test]
fn watch_all_files_off_leaves_only_config_and_schema_watchers() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    write_workspace(&base, "schema.graphql");
    let config = Config::load_from_dir(&base)
        .unwrap()
        .unwrap()
        .with_watch_all_files(false);

    let watchers = graphox_lsp::backend::file_watchers::build_file_watchers(&config, true);
    // Both config file names and the schema.
    assert_eq!(watchers.len(), 3, "{watchers:?}");
    assert!(
        !watchers
            .iter()
            .any(|w| w.glob_pattern == GlobPattern::String(ALL_FILES_GLOB.to_string()))
    );
}

// Applying watched-file events ------------------------------------------------

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn files_without_graphql_are_not_a_workspace_change() {
    let dir = workspace(&[
        ("src/fragment.graphql", "fragment F on User { id }"),
        ("src/util.ts", "export const one = 1;\n"),
    ]);
    let base = dir.path().canonicalize().unwrap();
    let service = scanned_service(&base).await;
    let backend = service.inner();
    let version = backend.workspace_version.load(Ordering::SeqCst);

    fs::write(base.join("src/util.ts"), "export const two = 2;\n").unwrap();
    let deleted = FileEvent {
        uri: graphox::utils::path_to_uri(base.join("src/gone.ts")).unwrap(),
        typ: FileChangeType::DELETED,
    };
    process_watched_file_batch(backend, vec![changed(&base, "src/util.ts"), deleted]).await;

    assert_eq!(backend.workspace_version.load(Ordering::SeqCst), version);
    let util = graphox::utils::path_to_uri(base.join("src/util.ts")).unwrap();
    assert!(!backend.metadata.contains_key(&util));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_host_file_that_loses_its_graphql_leaves_the_indices() {
    let dir = workspace(&[(
        "src/fragment.ts",
        "import { gql } from '@apollo/client';\nexport const F = gql`fragment HostFragment on User { id }`;\n",
    )]);
    let base = dir.path().canonicalize().unwrap();
    let service = scanned_service(&base).await;
    let backend = service.inner();
    assert!(is_defined(backend, "HostFragment"));
    let version = backend.workspace_version.load(Ordering::SeqCst);

    fs::write(base.join("src/fragment.ts"), "export const F = 1;\n").unwrap();
    process_watched_file_batch(backend, vec![changed(&base, "src/fragment.ts")]).await;

    assert!(!is_defined(backend, "HostFragment"));
    let uri = graphox::utils::path_to_uri(base.join("src/fragment.ts")).unwrap();
    assert!(!backend.metadata.contains_key(&uri));
    assert!(backend.workspace_version.load(Ordering::SeqCst) > version);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_host_file_that_gains_graphql_is_indexed() {
    let dir = workspace(&[("src/util.ts", "export const one = 1;\n")]);
    let base = dir.path().canonicalize().unwrap();
    let service = scanned_service(&base).await;
    let backend = service.inner();

    fs::write(
        base.join("src/util.ts"),
        "import { gql } from '@apollo/client';\nexport const F = gql`fragment Gained on User { name }`;\n",
    )
    .unwrap();
    process_watched_file_batch(backend, vec![changed(&base, "src/util.ts")]).await;

    assert!(is_defined(backend, "Gained"));
}
