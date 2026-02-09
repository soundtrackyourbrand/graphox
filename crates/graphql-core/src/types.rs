use ahash::{AHashSet, RandomState};
use dashmap::DashMap;
use lsp_types::{Diagnostic, Url};
use std::path::PathBuf;
use std::sync::Arc;

use crate::document::DocumentState;

pub type DocumentsMap = Arc<DashMap<Url, Arc<DocumentState>, RandomState>>;
pub type FragmentDefsMap = Arc<DashMap<Url, Vec<crate::document::FragmentDef>, RandomState>>;
pub type FragmentSpreadsMap = Arc<DashMap<Url, Vec<Arc<str>>, RandomState>>;
pub type PackageRootsMap = Arc<DashMap<Url, Option<PathBuf>, RandomState>>;
pub type FragmentDependentsMap = Arc<DashMap<Arc<str>, AHashSet<Url>, RandomState>>;
pub type FragmentDefinitionsMap = Arc<DashMap<Arc<str>, AHashSet<Url>, RandomState>>;
pub type OperationNamesMap = Arc<DashMap<Arc<str>, Vec<(Arc<str>, Url)>, RandomState>>;
pub type DiagnosticCacheMap = Arc<DashMap<Url, (i32, Vec<Diagnostic>), RandomState>>;
