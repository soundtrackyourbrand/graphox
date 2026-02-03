use graphql_rust::{Backend, Config};
use tower_lsp::{LspService, Server};

pub async fn run_lsp(config: Config) {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = LspService::new(|client| Backend::new(client, config));
    Server::new(stdin, stdout, socket).serve(service).await;
}
