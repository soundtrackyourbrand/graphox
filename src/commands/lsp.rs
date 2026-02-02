use graphql_rust::Backend;
use tower_lsp::{LspService, Server};

pub async fn run_lsp(schema_path: &str) {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Backend::new(client, schema_path));
    Server::new(stdin, stdout, socket).serve(service).await;
}
