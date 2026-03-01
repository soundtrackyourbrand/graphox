use crate::backend::handlers::{
    actions, completion, diagnostics, document_sync, hierarchy, hover, navigation, symbols,
};
use crate::backend::state::Backend;
use graphox_core::Config;
use graphox_features::signature_help::DocumentSignatureHelp;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{LanguageServer, LspService, Server};

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let caps = crate::backend::state::ClientCapabilities::from_params(&params);

        if let Ok(mut stored_caps) = self.client_capabilities.write() {
            *stored_caps = caps.clone();
        }

        self.client
            .log_message(
                MessageType::INFO,
                format!("Client capabilities: {}", caps.to_log_string()),
            )
            .await;

        Ok(InitializeResult {
            capabilities: super::capabilities::build_server_capabilities(&caps),
            server_info: Some(ServerInfo {
                name: "graphox-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "LSP Started!")
            .await;

        let config = self.config.read().unwrap().clone();

        let (supports_progress, position_encoding, supports_pull_diagnostics) =
            if let Ok(caps) = self.client_capabilities.read() {
                (
                    caps.supports_progress,
                    caps.negotiated_encoding(),
                    caps.supports_pull_diagnostics,
                )
            } else {
                (false, PositionEncodingKind::UTF16, false)
            };

        super::workspace_scan::spawn_workspace_scan(super::workspace_scan::WorkspaceScanParams {
            client: self.client.clone(),
            config: config.clone(),
            supports_pull_diagnostics,
            documents: self.documents.clone(),
            metadata: self.metadata.clone(),
            fragment_dependents: self.fragment_dependents.clone(),
            fragment_definitions: self.fragment_definitions.clone(),
            operation_names: self.operation_names.clone(),
            workspace_loaded: self.workspace_loaded.clone(),
            codegen_requested_during_scan: self.codegen_requested_during_scan.clone(),
            trigger_codegen_after_scan: None,
            empty_schema: self.empty_schema.clone(),
            schemas: self.schemas.clone(),
            subgraphs: self.subgraphs.clone(),
            validated_schemas: self.validated_schemas.clone(),
            workspace_scan_cancelled: self.workspace_scan_cancelled.read().unwrap().clone(),
            codegen_throttle: self.codegen_throttle.clone(),
            supports_progress,
            bypass_cache: false,
            fragment_metadata_cache: self.fragment_metadata_cache.clone(),
            position_encoding,
            workspace_version: self.workspace_version.clone(),
            last_full_validation_version: self.last_full_validation_version.clone(),
            open_documents: self.open_documents.clone(),
        });
    }

    async fn shutdown(&self) -> Result<()> {
        self.workspace_scan_cancelled
            .read()
            .unwrap()
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        document_sync::handle_did_open(self, params).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        document_sync::handle_did_change(self, params).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        document_sync::handle_did_save(self, params).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        document_sync::handle_did_close(self, params).await;
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        document_sync::handle_did_change_watched_files(self, params).await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        hover::handle_hover(self, params).await
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        completion::handle_completion(self, params).await
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        navigation::handle_goto_definition(self, params).await
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        navigation::handle_references(self, params).await
    }

    async fn goto_type_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        navigation::handle_goto_type_definition(self, params).await
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        symbols::handle_document_symbol(self, params).await
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        symbols::handle_workspace_symbol(self, params).await
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        actions::handle_code_action(self, params).await
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        actions::handle_execute_command(self, params).await
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = self.normalize_uri(params.text_document_position_params.text_document.uri);
        let position = params.text_document_position_params.position;

        let doc = if let Some(d) = self.documents.get(&uri).map(|r| r.value().clone()) {
            d
        } else {
            return Ok(None);
        };

        let schema = self.get_schema_for_doc(&uri);
        Ok(doc.get_signature_help(position, &schema))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        symbols::handle_semantic_tokens_full(self, params).await
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        navigation::handle_folding_range(self, params).await
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        navigation::handle_selection_range(self, params).await
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        navigation::handle_document_highlight(self, params).await
    }

    async fn diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReportResult> {
        diagnostics::handle_diagnostic(self, params).await
    }

    async fn workspace_diagnostic(
        &self,
        params: WorkspaceDiagnosticParams,
    ) -> Result<WorkspaceDiagnosticReportResult> {
        diagnostics::handle_workspace_diagnostic(self, params).await
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        navigation::handle_prepare_rename(self, params).await
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        navigation::handle_rename(self, params).await
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        hierarchy::handle_incoming_calls(self, params).await
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        hierarchy::handle_outgoing_calls(self, params).await
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        hierarchy::handle_prepare_call_hierarchy(self, params).await
    }
}

pub async fn run_lsp(config: Config) {
    let (service, socket) = LspService::new(|client| Backend::new(client, config));
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use graphox_core::config::{GlobPattern as GqlGlobPattern, ProjectConfig, SchemaSource};
    use std::sync::Arc;
    use tokio::time::{Duration, timeout};
    use tower_lsp::LspService;

    #[tokio::test]
    #[ntest::timeout(3000)]
    async fn test_validate_all_documents_performance() {
        let config = Config::new_test(
            std::env::current_dir().unwrap(),
            vec![
                ProjectConfig::default()
                    .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                    .with_include(GqlGlobPattern::Single("**/*.graphql".to_string())),
            ],
        );

        let (service, _) = LspService::new(|client| Backend::new(client, config));

        // This should complete very quickly even with multiple documents
        let res = timeout(
            Duration::from_millis(500),
            service.inner().validate_all_documents(),
        )
        .await;
        assert!(
            res.is_ok(),
            "validate_all_documents took too long or deadlocked"
        );
    }

    #[tokio::test]
    #[ntest::timeout(3000)]
    async fn test_get_all_fragments_info_no_deadlock() {
        let config = Config::new_test(std::env::current_dir().unwrap(), vec![]);

        let (service, _) = LspService::new(|client| Backend::new(client, config));
        let backend = service.inner();

        // Simulate some data
        let uri = Url::parse("file:///test.graphql").unwrap();
        let metadata = Arc::new(graphox_core::types::DocumentMetadata {
            fragments: Arc::from([]),
            fragment_spreads: Arc::from([]),
            package_root: None,
            operations: Arc::from([]),
            version: 0,
        });
        backend.metadata.insert(uri.clone(), metadata);

        let res = timeout(Duration::from_millis(100), async {
            backend.get_all_fragments_info()
        })
        .await;
        assert!(res.is_ok(), "get_all_fragments_info deadlocked");
    }
}
