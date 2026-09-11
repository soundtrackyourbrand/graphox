//! Workspace-wide analyses that look across documents rather than within one.
//!
//! Each analysis parses the same inputs — one file per [`DocumentSource`], with
//! host-language code masked out — and reports against the same vocabulary of
//! definitions, so their answers can be read side by side.

use std::path::{Path, PathBuf};

pub mod operations;
pub mod repeated_selections;
pub mod spreads;
pub mod usage;

/// A file to analyse, as its GraphQL source with host-language code masked out.
pub struct DocumentSource<'a> {
    pub path: &'a Path,
    pub project_idx: usize,
    pub source: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionKind {
    Operation,
    Fragment,
}

/// An operation or fragment, and where it was written.
#[derive(Debug, Clone)]
pub struct Definition {
    pub name: String,
    pub kind: DefinitionKind,
    pub path: PathBuf,
    pub project_idx: usize,
}
