//! Backend module organization
//!
//! This module is being refactored to reduce the size of a single 2153-line file
//! by extracting cohesive submodules for different responsibilities.

pub mod capabilities;
pub mod codegen_runner;
pub mod codegen_throttle;
pub mod document_changes;
pub mod error_logging;
pub mod file_change_handler;
pub mod file_watchers;
pub mod fragment_manager;
pub mod helpers;
pub mod lsp;
pub mod progress;
pub mod schema_management;
pub mod validation;
pub mod watched_files_debouncer;
pub mod workspace_scan;

pub mod handlers;
pub mod state;

// Re-export the main Backend struct from state
pub use state::Backend;
