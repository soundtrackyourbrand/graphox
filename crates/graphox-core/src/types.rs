use ahash::{AHashSet, RandomState};
use dashmap::DashMap;
use lsp_types::{Diagnostic, Url};
use std::path::PathBuf;
use std::sync::Arc;

use crate::document::DocumentState;

pub type DocumentsMap = Arc<DashMap<Url, Arc<DocumentState>, RandomState>>;
pub type MetadataMap = Arc<DashMap<Url, Arc<DocumentMetadata>, RandomState>>;
pub type FragmentDefinitionsMap = Arc<DashMap<Arc<str>, AHashSet<Url>, RandomState>>;
pub type FragmentDependentsMap = Arc<DashMap<Arc<str>, AHashSet<Url>, RandomState>>;
pub type OperationNamesMap = Arc<DashMap<Arc<str>, Vec<(Arc<str>, Url)>, RandomState>>;
pub type DiagnosticCacheEntry = (i32, usize, Vec<Diagnostic>);
pub type DiagnosticCacheMap = Arc<DashMap<Url, DiagnosticCacheEntry, RandomState>>;

#[derive(Debug, Clone)]
pub struct DocumentMetadata {
    pub fragments: Arc<[crate::document::FragmentDef]>,
    pub fragment_spreads: Arc<[Arc<str>]>,
    pub package_root: Option<PathBuf>,
    pub operations: Arc<[crate::document::OperationDef]>,
    pub version: i32,
}
