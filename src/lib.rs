pub mod backend;
pub mod config;
pub mod document;
pub mod engine;
pub mod features;
pub mod queries;
pub mod schema;
pub mod schema_cache;
pub mod types;
pub mod utils;

pub use backend::Backend;
pub use config::Config;
pub use document::{DocumentLanguage, DocumentState};

// Re-export commonly used schema cache functions
pub use schema_cache::clear_cache as clear_schema_cache;
