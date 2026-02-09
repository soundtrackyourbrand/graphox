use tower_lsp::lsp_types::*;

/// Client capabilities extracted from initialization
#[derive(Debug, Clone, Default)]
pub struct ClientCapabilities {
    pub supports_pull_diagnostics: bool,
    pub supports_workspace_folders: bool,
    pub supports_configuration: bool,
    pub supports_progress: bool,
    pub supports_semantic_tokens: bool,
    pub supports_inlay_hints: bool,
}

impl ClientCapabilities {
    /// Extract client capabilities from LSP initialization parameters
    pub fn from_params(params: &InitializeParams) -> Self {
        let client_caps = &params.capabilities;
        let mut caps = Self::default();

        // Check for pull diagnostics support (LSP 3.17+)
        if let Some(text_document) = &client_caps.text_document {
            caps.supports_pull_diagnostics = text_document.diagnostic.is_some();
        }

        // Check for workspace folder support
        if let Some(workspace) = &client_caps.workspace {
            caps.supports_workspace_folders = workspace.workspace_folders.unwrap_or(false);
            caps.supports_configuration = workspace.configuration.unwrap_or(false);
        }

        // Check for progress support
        if let Some(window) = &client_caps.window {
            caps.supports_progress = window.work_done_progress.unwrap_or(false);
        }

        // Check for semantic tokens support
        if let Some(text_document) = &client_caps.text_document {
            caps.supports_semantic_tokens = text_document.semantic_tokens.is_some();
        }

        // Check for inlay hints support (for future implementation)
        if let Some(text_document) = &client_caps.text_document {
            caps.supports_inlay_hints = text_document.inlay_hint.is_some();
        }

        caps
    }

    /// Format capabilities as a log-friendly string
    pub fn to_log_string(&self) -> String {
        format!(
            "pull_diagnostics={}, workspace_folders={}, progress={}, semantic_tokens={}, inlay_hints={}",
            self.supports_pull_diagnostics,
            self.supports_workspace_folders,
            self.supports_progress,
            self.supports_semantic_tokens,
            self.supports_inlay_hints,
        )
    }
}

/// Build server capabilities based on client capabilities
pub fn build_server_capabilities(client_caps: &ClientCapabilities) -> ServerCapabilities {
    use graphox_core::utils::SEMANTIC_TOKEN_LEGEND;

    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::INCREMENTAL,
        )),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        workspace: Some(WorkspaceServerCapabilities {
            workspace_folders: None,
            file_operations: None,
        }),
        semantic_tokens_provider: if client_caps.supports_semantic_tokens {
            Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
                SemanticTokensOptions {
                    work_done_progress_options: WorkDoneProgressOptions {
                        work_done_progress: None,
                    },
                    legend: SemanticTokensLegend {
                        token_types: SEMANTIC_TOKEN_LEGEND.to_vec(),
                        token_modifiers: vec![],
                    },
                    range: Some(false),
                    full: Some(SemanticTokensFullOptions::Bool(true)),
                },
            ))
        } else {
            None
        },
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            trigger_characters: Some(vec![".".to_string()]),
            ..Default::default()
        }),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
            retrigger_characters: None,
            work_done_progress_options: Default::default(),
        }),
        call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: vec![
                "graphql.runCodegen".to_string(),
                "graphql.clearCache".to_string(),
            ],
            ..Default::default()
        }),
        diagnostic_provider: if client_caps.supports_pull_diagnostics {
            Some(DiagnosticServerCapabilities::Options(DiagnosticOptions {
                identifier: Some("graphox".to_string()),
                inter_file_dependencies: true,
                workspace_diagnostics: true,
                work_done_progress_options: Default::default(),
            }))
        } else {
            None
        },
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        ..Default::default()
    }
}
