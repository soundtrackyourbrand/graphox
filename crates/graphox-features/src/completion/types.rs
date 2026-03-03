use lsp_types::Url;
use std::sync::Arc;

pub type FragmentRequirements = std::collections::BTreeMap<Arc<str>, Arc<str>>;
pub type FragmentRequirementsResolver = Arc<dyn Fn(&str) -> FragmentRequirements + Send + Sync>;

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
    pub used_variables: Arc<[Arc<str>]>,
    pub used_fragments: Arc<[Arc<str>]>,
    pub transitive_deps: Arc<[Arc<str>]>,
    pub selected_fields: Arc<[Arc<str>]>,
    pub type_fields: Arc<[(Arc<str>, Arc<str>)]>,
    pub requirements: FragmentRequirements,
    pub worst_slo: Option<graphox_core::schema::SloClass>,
}
