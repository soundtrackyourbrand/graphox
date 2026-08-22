//! LSP-specific test utilities for reducing boilerplate in integration tests.

use crate::support::lsp_did_open;
use crate::support::lsp_initialize_sequence;
use crate::support::lsp_send_notification;
use graphox::Backend;
pub type LspBackend = graphox::GraphoxLanguageServer;
use std::path::Path;
use tempfile::TempDir;
use tower_lsp_server::LspService;
use tower_lsp_server::ls_types::*;

// =============================================================================
// LSP Test Helper
// =============================================================================

/// Helper for creating LSP test scenarios with multiple files.
///
/// # Example
///
/// ```
/// let scenario = LspTestScenario::new()
///     .with_file("schema.graphql", "type Query { user: User }")
///     .with_file("query.graphql", "query { user { id } }")
///     .initialize()
///     .await;
/// ```
pub struct LspTestScenario {
    temp_dir: TempDir,
    files: Vec<(String, String)>, // (relative_path, content)
    config: Option<graphox::Config>,
}

impl LspTestScenario {
    /// Create a new empty scenario.
    pub fn new() -> Self {
        Self {
            temp_dir: TempDir::new().unwrap(),
            files: Vec::new(),
            config: None,
        }
    }

    /// Add a file to the scenario.
    pub fn with_file(mut self, path: &str, content: &str) -> Self {
        self.files.push((path.to_string(), content.to_string()));
        self
    }

    /// Set a custom config.
    pub fn with_config(mut self, config: graphox::Config) -> Self {
        self.config = Some(config);
        self
    }

    /// Write all files to disk and return the base directory.
    pub fn write_files(&self) -> std::io::Result<std::path::PathBuf> {
        let base_dir = self.temp_dir.path().canonicalize()?;

        for (path, content) in &self.files {
            let file_path = base_dir.join(path);
            if let Some(parent) = file_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&file_path, content)?;
        }

        Ok(base_dir)
    }

    /// Build the config with all files included.
    pub fn build_config(&self, base_dir: &Path) -> graphox::Config {
        if let Some(config) = &self.config {
            return config.clone();
        }

        let schemas: Vec<String> = self
            .files
            .iter()
            .filter(|(p, _)| p.ends_with(".graphql") && p.contains("schema"))
            .map(|(p, _)| p.to_string())
            .collect();

        let includes: Vec<String> = self
            .files
            .iter()
            .filter(|(p, _)| !p.ends_with("schema.graphql"))
            .map(|(p, _)| format!("**/{}", p))
            .collect();

        let mut projects = Vec::new();

        if !schemas.is_empty() {
            let include = if includes.is_empty() {
                "**/*.graphql".to_string()
            } else {
                includes.join(", ")
            };

            projects.push(
                crate::support::ProjectConfigBuilder::new()
                    .multi_schema(schemas)
                    .include_pattern(&include)
                    .codegen(false)
                    .build(),
            );
        }

        graphox::Config::new_test(base_dir.to_path_buf(), projects)
            .with_enable_schema_cache(false)
            .with_lsp_automatic_codegen(false)
    }

    /// Initialize the LSP service and return a helper.
    pub async fn initialize(self) -> LspTestInitialized {
        let base_dir = self.write_files().unwrap();
        let config = self.build_config(&base_dir);

        let (mut service, _) = LspService::new(|client| {
            graphox::GraphoxLanguageServer::new(Backend::new(client, config))
        });
        lsp_initialize_sequence(&mut service).await;

        // Open all files
        for (path, content) in self.files {
            let file_path = base_dir.join(&path);
            let uri = graphox::utils::path_to_uri(file_path).unwrap();
            lsp_did_open(&mut service, uri.clone(), "graphql", 1, &content).await;
        }

        LspTestInitialized { service, base_dir }
    }
}

impl Default for LspTestScenario {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Initialized LSP Test Helper
// =============================================================================

/// LSP service that has been initialized and has all files opened.
pub struct LspTestInitialized {
    service: LspService<LspBackend>,
    base_dir: std::path::PathBuf,
}

impl LspTestInitialized {
    /// Get the LSP service.
    pub fn service(&mut self) -> &mut LspService<LspBackend> {
        &mut self.service
    }

    /// Get the base directory.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    /// Get a URI for a file in the scenario.
    pub fn uri_for(&self, path: &str) -> Uri {
        let file_path = self.base_dir.join(path);
        graphox::utils::path_to_uri(file_path).unwrap()
    }

    /// Update a file by sending a didChange notification.
    pub async fn update_file(&mut self, path: &str, new_content: &str) {
        let uri = self.uri_for(path);
        let params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: new_content.to_string(),
            }],
        };
        lsp_send_notification(&mut self.service, "textDocument/didChange", &params).await;
    }

    /// Read a file's content.
    pub fn read_file(&self, path: &str) -> String {
        std::fs::read_to_string(self.base_dir.join(path)).unwrap()
    }
}
