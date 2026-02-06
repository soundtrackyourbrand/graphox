//! Error logging utilities for the LSP
//!
//! This module provides utilities for consistent error logging to the LSP client.

use tower_lsp::Client;
use tower_lsp::lsp_types::MessageType;

/// Logs an error message to the LSP client
pub async fn log_error(client: &Client, context: &str, error: impl std::fmt::Display) {
    client
        .log_message(MessageType::ERROR, format!("{}: {}", context, error))
        .await;
}

/// Logs a warning message to the LSP client
pub async fn log_warning(client: &Client, context: &str, message: impl std::fmt::Display) {
    client
        .log_message(MessageType::WARNING, format!("{}: {}", context, message))
        .await;
}

/// Helper trait for logging errors from Results
pub trait LogError<T> {
    /// Log the error if Result is Err, then return the Result unchanged
    fn log_err(
        self,
        client: &Client,
        context: &str,
    ) -> impl std::future::Future<Output = Self> + Send;

    /// Log the error if Result is Err, then convert to Option
    fn log_err_opt(
        self,
        client: &Client,
        context: &str,
    ) -> impl std::future::Future<Output = Option<T>> + Send;
}

impl<T: Send, E: std::fmt::Display + Send + Sync> LogError<T> for Result<T, E> {
    fn log_err(
        self,
        client: &Client,
        context: &str,
    ) -> impl std::future::Future<Output = Self> + Send {
        let context = context.to_string();
        async move {
            if let Err(ref e) = self {
                log_error(client, &context, e).await;
            }
            self
        }
    }

    fn log_err_opt(
        self,
        client: &Client,
        context: &str,
    ) -> impl std::future::Future<Output = Option<T>> + Send {
        let context = context.to_string();
        async move {
            match self {
                Ok(v) => Some(v),
                Err(e) => {
                    log_error(client, &context, e).await;
                    None
                }
            }
        }
    }
}

/// Synchronous error logging for use in non-async contexts
pub fn log_error_sync(message: String) {
    eprintln!("[LSP ERROR] {}", message);
}

/// Synchronous warning logging for use in non-async contexts
pub fn log_warning_sync(message: String) {
    eprintln!("[LSP WARNING] {}", message);
}
