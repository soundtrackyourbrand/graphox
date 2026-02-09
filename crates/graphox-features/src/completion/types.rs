use lsp_types::Url;
use std::sync::Arc;

#[derive(Clone)]
pub struct FragmentCompletionInfo {
    pub name: Arc<str>,
    pub type_condition: Arc<str>,
    pub description: Option<Arc<str>>,
    pub import_path: Option<Arc<str>>,
    pub is_public: bool,
    pub is_type_only: bool,
    pub uri: Url,
    pub package_root: Option<std::path::PathBuf>,
    pub used_variables: Vec<Arc<str>>,
    pub used_fragments: Vec<Arc<str>>,
    pub requirements: std::collections::BTreeMap<Arc<str>, Arc<str>>,
}
