//! Backend module organization
//!
//! This module is being refactored to reduce the size of a single 2153-line file
//! by extracting cohesive submodules for different responsibilities.

pub mod fragment_manager;
mod lsp;

// Re-export the main Backend struct and its implementation
pub use lsp::Backend;
