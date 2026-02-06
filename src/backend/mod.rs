//! Backend module organization
//!
//! This module is being refactored to reduce the size of a single 2153-line file
//! by extracting cohesive submodules for different responsibilities.

pub mod codegen_runner;
pub mod document_changes;
pub mod file_change_handler;
pub mod file_watchers;
pub mod fragment_manager;
pub mod progress;
pub mod schema_management;
pub mod validation;
pub mod workspace_scan;
mod lsp;

// Re-export the main Backend struct and its implementation
pub use lsp::Backend;
