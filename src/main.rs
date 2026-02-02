use tower_lsp::{LspService, Server};
use graphql_rust::Backend;

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let schema_path = "";
    let (service, socket) = LspService::new(|client| Backend::new(client, schema_path));
    Server::new(stdin, stdout, socket).serve(service).await;
}