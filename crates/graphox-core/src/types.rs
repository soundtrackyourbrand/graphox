use ahash::{AHashSet, RandomState};
use dashmap::DashMap;
use ls_types::{Diagnostic, Uri};
use std::path::PathBuf;
use std::sync::Arc;

use crate::document::DocumentState;

pub type DocumentsMap = Arc<DashMap<Uri, Arc<DocumentState>, RandomState>>;
pub type MetadataMap = Arc<DashMap<Uri, Arc<DocumentMetadata>, RandomState>>;
pub type FragmentDefinitionsMap = Arc<DashMap<Arc<str>, AHashSet<Uri>, RandomState>>;
pub type FragmentDependentsMap = Arc<DashMap<Arc<str>, AHashSet<Uri>, RandomState>>;
pub type OperationNamesMap = Arc<DashMap<Arc<str>, Vec<(Arc<str>, Uri)>, RandomState>>;
pub type DiagnosticCacheEntry = (i32, usize, Vec<Diagnostic>);
pub type DiagnosticCacheMap = Arc<DashMap<Uri, DiagnosticCacheEntry, RandomState>>;

#[derive(Debug, Clone)]
pub struct DocumentMetadata {
    pub fragments: Arc<[crate::document::FragmentDef]>,
    pub fragment_spreads: Arc<[Arc<str>]>,
    pub package_root: Option<PathBuf>,
    pub operations: Arc<[crate::document::OperationDef]>,
    pub version: i32,
}
