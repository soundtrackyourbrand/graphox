use std::time::Instant;
use tower_lsp::Client;
use tower_lsp::lsp_types::*;

/// Normalize a file URI by resolving it to canonical path
pub fn normalize_uri(uri: Url) -> Url {
    graphox_core::utils::normalize_uri(uri)
}

/// Execute an async operation with tracing and timeout support
pub async fn with_tracing<T, Fut>(
    client: &Client,
    name: &str,
    timeout_ms: u64,
    tracing_config: Option<(bool, u64)>,
    fut: Fut,
) -> tower_lsp::jsonrpc::Result<Option<T>>
where
    Fut: std::future::Future<Output = tower_lsp::jsonrpc::Result<Option<T>>>,
{
    let start = Instant::now();

    // Apply timeout
    let res = match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), fut).await {
        Ok(result) => result,
        Err(_) => {
            let elapsed = start.elapsed();
            client
                .log_message(
                    MessageType::ERROR,
                    format!(
                        "LSP Request '{}' exceeded timeout of {}ms (took {}ms) - returning empty response",
                        name,
                        timeout_ms,
                        elapsed.as_millis()
                    ),
                )
                .await;
            // Return Ok(None) for timed out requests
            Ok(None)
        }
    };

    // Log if tracing is enabled and threshold exceeded
    if let Some((enabled, threshold_ms)) = tracing_config
        && enabled
    {
        let elapsed = start.elapsed();
        if elapsed.as_millis() >= threshold_ms as u128 {
            client
                .log_message(
                    MessageType::INFO,
                    format!("LSP Request '{}' took {}ms", name, elapsed.as_millis()),
                )
                .await;
        }
    }

    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ntest::timeout(3000)]
    fn test_normalize_uri_preserves_valid_uri() {
        let uri = Url::parse("file:///tmp/test.graphql").unwrap();
        let normalized = normalize_uri(uri.clone());
        assert!(normalized.scheme() == "file");
    }
}
