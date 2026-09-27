//! File watcher registration for LSP
//!
//! The server asks the client to watch the config file, the schema files and,
//! unless `watch_all_files` is off, every file that can hold GraphQL. Changes
//! made outside the editor (a checkout, a pull, a rebase, a codegen run from a
//! terminal) reach the server only through these watchers.

use ahash::AHashSet;
use graphox_core::Config;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tower_lsp_server::Client;
use tower_lsp_server::ls_types::*;

use super::capabilities::ClientCapabilities;

const REGISTRATION_ID: &str = "watch-files";
const WATCHED_FILES_METHOD: &str = "workspace/didChangeWatchedFiles";

/// Builds the watchers for `config`.
///
/// Schema paths in the config are relative to its directory, but clients match
/// a plain glob string against absolute paths, so a relative string would
/// never fire. They are sent as relative patterns where the client supports
/// them, and as absolute paths otherwise.
pub fn build_file_watchers(config: &Config, relative_patterns: bool) -> Vec<FileSystemWatcher> {
    let base_dir = config.base_dir();
    let watch = |glob_pattern| FileSystemWatcher {
        glob_pattern,
        kind: Some(WatchKind::all()),
    };
    let mut watchers = Vec::new();

    // Both names, whichever exists: creating the preferred `graphox.yaml` next
    // to a `graphox.yml` switches the config, and only a watcher on the new
    // file reports that.
    for name in ["graphox.yaml", "graphox.yml"] {
        watchers.push(watch(GlobPattern::String(
            base_dir.join(name).to_string_lossy().into_owned(),
        )));
    }

    let mut schema_files = AHashSet::default();
    let mut ordered = Vec::new();
    let schema_sources = config
        .projects()
        .iter()
        .map(|p| p.schema())
        .chain(config.schema_types().iter().map(|st| st.schema()));
    for schema in schema_sources {
        for file in schema.files() {
            if schema_files.insert(file.clone()) {
                ordered.push(file);
            }
        }
    }
    let base_uri = graphox_core::utils::path_to_uri(base_dir);
    for file in ordered {
        let pattern = match (&base_uri, relative_patterns) {
            (Some(base_uri), true) => GlobPattern::Relative(RelativePattern {
                base_uri: OneOf::Right(base_uri.clone()),
                pattern: file,
            }),
            _ => GlobPattern::String(base_dir.join(file).to_string_lossy().into_owned()),
        };
        watchers.push(watch(pattern));
    }

    if config.watch_all_files() {
        watchers.push(watch(GlobPattern::String(
            "**/*.{graphql,gql,ts,tsx,mts,cts,js,jsx,mjs,cjs}".to_string(),
        )));
    }

    watchers
}

/// The watcher registration for `config`, or `None` for a client that cannot
/// register watchers dynamically: the protocol forbids registering a
/// capability the client did not offer to let the server register.
pub fn watcher_registration(
    config: &Config,
    capabilities: &ClientCapabilities,
) -> Option<Registration> {
    if !capabilities.supports_watched_files_registration {
        return None;
    }
    let watchers = build_file_watchers(config, capabilities.supports_relative_watch_patterns);
    Some(Registration {
        id: REGISTRATION_ID.to_string(),
        method: WATCHED_FILES_METHOD.to_string(),
        register_options: Some(
            serde_json::to_value(DidChangeWatchedFilesRegistrationOptions { watchers }).unwrap(),
        ),
    })
}

/// Registers the watchers with the client, one registration at a time.
///
/// A config reload replaces the registration, and the client answers each
/// request asynchronously, so two reloads close together could otherwise
/// interleave their unregister and register requests and leave the older
/// config's watchers registered. Requests are applied in order, and one that a
/// newer request has already superseded is skipped.
#[derive(Default)]
pub struct WatcherRegistrar {
    requested: AtomicU64,
    /// Whether a registration is live on the client, and so must be dropped
    /// before the next one.
    registered: tokio::sync::Mutex<bool>,
}

impl WatcherRegistrar {
    /// Registers the watchers for `config`, replacing any earlier registration.
    pub fn register(
        self: &Arc<Self>,
        client: Client,
        config: &Config,
        capabilities: &ClientCapabilities,
    ) {
        let Some(registration) = watcher_registration(config, capabilities) else {
            return;
        };
        let generation = self.requested.fetch_add(1, Ordering::SeqCst) + 1;
        let this = self.clone();

        tokio::spawn(async move {
            let mut registered = this.registered.lock().await;
            if this.requested.load(Ordering::SeqCst) != generation {
                return;
            }
            if *registered {
                let unregistration = Unregistration {
                    id: REGISTRATION_ID.to_string(),
                    method: WATCHED_FILES_METHOD.to_string(),
                };
                if let Err(e) = client.unregister_capability(vec![unregistration]).await {
                    client
                        .log_message(
                            MessageType::WARNING,
                            format!("Failed to unregister file watchers: {e}"),
                        )
                        .await;
                }
                *registered = false;
            }
            match client.register_capability(vec![registration]).await {
                Ok(()) => *registered = true,
                Err(e) => {
                    client
                        .log_message(
                            MessageType::ERROR,
                            format!("Failed to register file watchers: {e}"),
                        )
                        .await;
                }
            }
        });
    }
}
