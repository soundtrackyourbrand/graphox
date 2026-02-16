use ahash::AHashMap;
use colored::*;
use dashmap::DashMap;
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};
use yaml_rust2::Yaml;

static GLOBSET_CACHE: LazyLock<DashMap<Vec<String>, Arc<GlobSet>>> = LazyLock::new(DashMap::new);

fn get_glob_set(patterns: &[String]) -> Arc<GlobSet> {
    if let Some(set) = GLOBSET_CACHE.get(patterns) {
        return set.value().clone();
    }

    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if let Ok(glob) = Glob::new(pattern) {
            builder.add(glob);
        }
    }
    let set = Arc::new(
        builder
            .build()
            .unwrap_or_else(|_| GlobSetBuilder::new().build().unwrap()),
    );
    GLOBSET_CACHE.insert(patterns.to_vec(), set.clone());
    set
}

pub fn clear_globset_cache() {
    GLOBSET_CACHE.clear();
}

#[derive(Debug, Clone, Default)]
pub struct RulesConfig {
    required_fields: Option<AHashMap<String, RequiredFieldRule>>,
    unique_operation_name: Option<bool>,
    no_duplicate_fields: Option<bool>,
    no_unused_fragments: Option<bool>,
}

impl RulesConfig {
    pub fn with_unique_operation_name(mut self, enabled: bool) -> Self {
        self.unique_operation_name = Some(enabled);
        self
    }

    pub fn with_no_duplicate_fields(mut self, enabled: bool) -> Self {
        self.no_duplicate_fields = Some(enabled);
        self
    }

    pub fn with_no_unused_fragments(mut self, enabled: bool) -> Self {
        self.no_unused_fragments = Some(enabled);
        self
    }

    pub fn with_required_fields(mut self, fields: AHashMap<String, RequiredFieldRule>) -> Self {
        self.required_fields = Some(fields);
        self
    }

    pub fn required_fields(&self) -> &AHashMap<String, RequiredFieldRule> {
        static EMPTY: LazyLock<AHashMap<String, RequiredFieldRule>> =
            LazyLock::new(AHashMap::default);
        self.required_fields.as_ref().unwrap_or(&EMPTY)
    }

    pub fn unique_operation_name(&self) -> bool {
        self.unique_operation_name.unwrap_or(false)
    }

    pub fn no_duplicate_fields(&self) -> bool {
        self.no_duplicate_fields.unwrap_or(false)
    }

    pub fn no_unused_fragments(&self) -> bool {
        self.no_unused_fragments.unwrap_or(false)
    }
}

#[derive(Debug, Clone)]
pub enum RequiredFieldRule {
    Always(bool),
    Operations(Vec<String>),
}

impl RequiredFieldRule {
    pub fn applies_to_operation(&self, operation_type: &str) -> bool {
        match self {
            RequiredFieldRule::Always(enabled) => *enabled,
            RequiredFieldRule::Operations(ops) => {
                ops.iter().any(|op| op.eq_ignore_ascii_case(operation_type))
            }
        }
    }

    fn from_yaml(node: &Yaml) -> Option<Self> {
        if let Some(b) = node.as_bool() {
            return Some(RequiredFieldRule::Always(b));
        }
        if let Some(v) = node.as_vec() {
            let ops = v
                .iter()
                .filter_map(|n| n.as_str().map(String::from))
                .collect();
            return Some(RequiredFieldRule::Operations(ops));
        }
        None
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Hash)]
pub enum EmitExtensions {
    #[default]
    None,
    Ts,
    Dts,
    Js,
}

impl EmitExtensions {
    pub fn as_str(&self) -> &str {
        match self {
            EmitExtensions::None => "",
            EmitExtensions::Ts => ".ts",
            EmitExtensions::Dts => ".d.ts",
            EmitExtensions::Js => ".js",
        }
    }

    fn from_yaml(node: &Yaml) -> Self {
        if let Some(s) = node.as_str() {
            match s.to_lowercase().as_str() {
                "ts" => EmitExtensions::Ts,
                "dts" | "d.ts" => EmitExtensions::Dts,
                "js" | "javascript" => EmitExtensions::Js,
                _ => EmitExtensions::None,
            }
        } else {
            EmitExtensions::None
        }
    }
}

#[derive(Debug, Clone, Default, Hash)]
pub struct CodegenConfig {
    pub enabled: Option<bool>,
    pub document_suffix: Option<String>,
    pub variables_suffix: Option<String>,
    pub fragment_suffix: Option<String>,
    pub fragment_document_suffix: Option<String>,
    pub query_suffix: Option<String>,
    pub mutation_suffix: Option<String>,
    pub subscription_suffix: Option<String>,
    pub naming_convention: Option<NamingConvention>,
    pub fragment_masking: Option<FragmentMaskingConfig>,
    pub emit_extensions: Option<EmitExtensions>,
    pub generate_ast_for_fragments: Option<bool>,
    pub re_exports: Option<bool>,
    pub emit_permission_data: Option<bool>,
    pub emit_ast_directives: Option<bool>,
    pub emit_ast_aliases: Option<bool>,
    pub emit_ast_arguments: Option<bool>,
    pub emit_ast_variable_defaults: Option<bool>,
    pub inline_fragments: Option<bool>,
    pub default_scalar_type: Option<String>,
    pub schema_import: Option<String>,
}

impl CodegenConfig {
    pub fn fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = ahash::AHasher::default();
        self.hash(&mut hasher);
        hasher.finish()
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    pub fn with_document_suffix(mut self, suffix: String) -> Self {
        self.document_suffix = Some(suffix);
        self
    }

    pub fn with_variables_suffix(mut self, suffix: String) -> Self {
        self.variables_suffix = Some(suffix);
        self
    }

    pub fn with_fragment_suffix(mut self, suffix: String) -> Self {
        self.fragment_suffix = Some(suffix);
        self
    }

    pub fn with_fragment_document_suffix(mut self, suffix: String) -> Self {
        self.fragment_document_suffix = Some(suffix);
        self
    }

    pub fn with_query_suffix(mut self, suffix: String) -> Self {
        self.query_suffix = Some(suffix);
        self
    }

    pub fn with_mutation_suffix(mut self, suffix: String) -> Self {
        self.mutation_suffix = Some(suffix);
        self
    }

    pub fn with_subscription_suffix(mut self, suffix: String) -> Self {
        self.subscription_suffix = Some(suffix);
        self
    }

    pub fn with_naming_convention(mut self, convention: NamingConvention) -> Self {
        self.naming_convention = Some(convention);
        self
    }

    pub fn with_fragment_masking(mut self, config: FragmentMaskingConfig) -> Self {
        self.fragment_masking = Some(config);
        self
    }

    pub fn with_emit_extensions(mut self, extensions: EmitExtensions) -> Self {
        self.emit_extensions = Some(extensions);
        self
    }

    pub fn with_generate_ast_for_fragments(mut self, enabled: bool) -> Self {
        self.generate_ast_for_fragments = Some(enabled);
        self
    }

    pub fn with_re_exports(mut self, enabled: bool) -> Self {
        self.re_exports = Some(enabled);
        self
    }

    pub fn with_emit_permission_data(mut self, enabled: bool) -> Self {
        self.emit_permission_data = Some(enabled);
        self
    }

    pub fn from_yaml(node: &Yaml) -> Option<Self> {
        if node.is_null() || node.is_badvalue() {
            return None;
        }

        // Handle legacy boolean format: codegen: true/false
        if let Some(b) = node.as_bool() {
            return Some(Self {
                enabled: Some(b),
                ..Default::default()
            });
        }

        Some(Self {
            enabled: node["enabled"].as_bool(),
            document_suffix: node["document_suffix"].as_str().map(String::from),
            variables_suffix: node["variables_suffix"].as_str().map(String::from),
            fragment_suffix: node["fragment_suffix"].as_str().map(String::from),
            fragment_document_suffix: node["fragment_document_suffix"].as_str().map(String::from),
            query_suffix: node["query_suffix"].as_str().map(String::from),
            mutation_suffix: node["mutation_suffix"].as_str().map(String::from),
            subscription_suffix: node["subscription_suffix"].as_str().map(String::from),
            naming_convention: NamingConvention::from_yaml(&node["naming_convention"]),
            fragment_masking: {
                let frag_masking_node = &node["fragment_masking"];
                FragmentMasking::from_yaml(frag_masking_node)
                    .map(|mode| FragmentMaskingConfig { mode })
            },
            emit_extensions: Some(EmitExtensions::from_yaml(&node["emit_extensions"])),
            generate_ast_for_fragments: node["generate_ast_for_fragments"].as_bool(),
            re_exports: node["re_exports"].as_bool(),
            emit_permission_data: node["emit_permission_data"].as_bool(),
            emit_ast_directives: node["emit_ast_directives"].as_bool(),
            emit_ast_aliases: node["emit_ast_aliases"].as_bool(),
            emit_ast_arguments: node["emit_ast_arguments"].as_bool(),
            emit_ast_variable_defaults: node["emit_ast_variable_defaults"].as_bool(),
            inline_fragments: node["inline_fragments"].as_bool(),
            default_scalar_type: node["default_scalar_type"].as_str().map(String::from),
            schema_import: node["schema_import"].as_str().map(String::from),
        })
    }

    pub fn schema_import(&self) -> Option<&str> {
        self.schema_import.as_deref()
    }

    pub fn default_scalar_type(&self) -> &str {
        self.default_scalar_type.as_deref().unwrap_or("any")
    }

    pub fn document_suffix(&self) -> &str {
        self.document_suffix.as_deref().unwrap_or("Document")
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }

    pub fn enabled() -> Self {
        Self {
            enabled: Some(true),
            ..Default::default()
        }
    }

    pub fn disabled() -> Self {
        Self {
            enabled: Some(false),
            ..Default::default()
        }
    }

    pub fn variables_suffix(&self) -> &str {
        self.variables_suffix.as_deref().unwrap_or("Variables")
    }

    pub fn fragment_suffix(&self) -> &str {
        self.fragment_suffix.as_deref().unwrap_or("")
    }

    pub fn fragment_document_suffix(&self) -> &str {
        self.fragment_document_suffix
            .as_deref()
            .or(self.document_suffix.as_deref())
            .unwrap_or("Document")
    }

    pub fn query_suffix(&self) -> &str {
        self.query_suffix.as_deref().unwrap_or("Query")
    }

    pub fn mutation_suffix(&self) -> &str {
        self.mutation_suffix.as_deref().unwrap_or("Mutation")
    }

    pub fn subscription_suffix(&self) -> &str {
        self.subscription_suffix
            .as_deref()
            .unwrap_or("Subscription")
    }

    pub fn naming_convention(&self) -> NamingConvention {
        self.naming_convention.clone().unwrap_or_default()
    }

    pub fn fragment_masking(&self) -> FragmentMaskingConfig {
        self.fragment_masking.clone().unwrap_or_default()
    }

    pub fn fragment_masking_mode(&self) -> FragmentMasking {
        self.fragment_masking
            .as_ref()
            .map(|c| c.mode.clone())
            .unwrap_or_default()
    }

    pub fn emit_extensions(&self) -> EmitExtensions {
        self.emit_extensions.unwrap_or(EmitExtensions::None)
    }

    pub fn generate_ast_for_fragments(&self) -> bool {
        self.generate_ast_for_fragments.unwrap_or(false)
    }

    pub fn re_exports(&self) -> bool {
        self.re_exports.unwrap_or(false)
    }

    pub fn emit_permission_data(&self) -> bool {
        self.emit_permission_data.unwrap_or(false)
    }

    pub fn emit_ast_directives(&self) -> bool {
        self.emit_ast_directives.unwrap_or(false)
    }

    pub fn emit_ast_aliases(&self) -> bool {
        self.emit_ast_aliases.unwrap_or(true)
    }

    pub fn emit_ast_arguments(&self) -> bool {
        self.emit_ast_arguments.unwrap_or(true)
    }

    pub fn emit_ast_variable_defaults(&self) -> bool {
        self.emit_ast_variable_defaults.unwrap_or(true)
    }

    pub fn inline_fragments(&self) -> bool {
        self.inline_fragments.unwrap_or(false)
    }
}

#[derive(Debug, Clone)]
pub struct TracingConfig {
    enabled: bool,
    threshold_ms: u64,
}

impl TracingConfig {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn threshold_ms(&self) -> u64 {
        self.threshold_ms
    }
}

#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    workspace_scan_ms: u64,
    lsp_request_ms: u64,
}

impl TimeoutConfig {
    pub fn with_workspace_scan_ms(mut self, ms: u64) -> Self {
        self.workspace_scan_ms = ms;
        self
    }

    pub fn with_lsp_request_ms(mut self, ms: u64) -> Self {
        self.lsp_request_ms = ms;
        self
    }

    pub fn workspace_scan_ms(&self) -> u64 {
        self.workspace_scan_ms
    }

    pub fn lsp_request_ms(&self) -> u64 {
        self.lsp_request_ms
    }
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            workspace_scan_ms: 60_000,
            lsp_request_ms: 1_000,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SchemaSource {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone, Default, Hash)]
pub enum FragmentMasking {
    #[default]
    Disabled,
    Enabled {
        unmask_function_name: Option<String>,
    },
}

impl FragmentMasking {
    pub fn is_enabled(&self) -> bool {
        matches!(self, FragmentMasking::Enabled { .. })
    }

    pub fn unmask_function_name(&self) -> &str {
        match self {
            FragmentMasking::Enabled {
                unmask_function_name: Some(name),
            } => name,
            _ => "getFragmentData",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Hash)]
pub enum NamingConvention {
    #[default]
    PascalCase,
    Preserve,
}

impl NamingConvention {
    pub fn from_yaml(node: &Yaml) -> Option<Self> {
        if node.is_null() || node.is_badvalue() {
            return None;
        }
        if let Some(s) = node.as_str() {
            match s.to_lowercase().as_str() {
                "pascal_case" | "pascalcase" | "pascal" => Some(NamingConvention::PascalCase),
                "preserve" => Some(NamingConvention::Preserve),
                _ => Some(NamingConvention::PascalCase),
            }
        } else {
            Some(NamingConvention::PascalCase)
        }
    }
}

impl FragmentMasking {
    pub fn from_yaml(node: &Yaml) -> Option<Self> {
        if node.is_null() || node.is_badvalue() {
            return None;
        }
        if let Some(b) = node.as_bool() {
            if b {
                Some(FragmentMasking::Enabled {
                    unmask_function_name: None,
                })
            } else {
                Some(FragmentMasking::Disabled)
            }
        } else if let Some(s) = node.as_str() {
            match s.to_lowercase().as_str() {
                "enabled" | "true" => Some(FragmentMasking::Enabled {
                    unmask_function_name: None,
                }),
                "disabled" | "false" => Some(FragmentMasking::Disabled),
                _ => Some(FragmentMasking::Disabled),
            }
        } else if let Some(map) = node.as_hash() {
            let unmask_function_name = map
                .get(&Yaml::String("unmask_function_name".to_string()))
                .and_then(|v| v.as_str())
                .map(String::from);
            Some(FragmentMasking::Enabled {
                unmask_function_name,
            })
        } else {
            Some(FragmentMasking::Disabled)
        }
    }
}

#[derive(Debug, Clone, Hash)]
pub struct FragmentMaskingConfig {
    mode: FragmentMasking,
}

impl Default for FragmentMaskingConfig {
    fn default() -> Self {
        Self {
            mode: FragmentMasking::Disabled,
        }
    }
}

impl FragmentMaskingConfig {
    pub fn mode(&self) -> FragmentMasking {
        self.mode.clone()
    }
}

impl Default for SchemaSource {
    fn default() -> Self {
        Self::Single("schema.graphql".to_string())
    }
}

impl SchemaSource {
    pub fn as_key(&self) -> String {
        match self {
            SchemaSource::Single(s) => s.clone(),
            SchemaSource::Multiple(v) => v.join(","),
        }
    }

    pub fn files(&self) -> Vec<String> {
        match self {
            SchemaSource::Single(s) => vec![s.clone()],
            SchemaSource::Multiple(v) => v.clone(),
        }
    }

    fn from_yaml(node: &Yaml) -> Option<Self> {
        if let Some(s) = node.as_str() {
            Some(SchemaSource::Single(s.to_string()))
        } else if let Some(v) = node.as_vec() {
            let files = v
                .iter()
                .filter_map(|n| n.as_str().map(String::from))
                .collect();
            Some(SchemaSource::Multiple(files))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone)]
pub enum GlobPattern {
    Single(String),
    Multiple(Vec<String>),
}

impl Default for GlobPattern {
    fn default() -> Self {
        Self::Single("**/*.graphql".to_string())
    }
}

impl GlobPattern {
    pub fn as_key(&self) -> String {
        match self {
            GlobPattern::Single(s) => s.clone(),
            GlobPattern::Multiple(v) => v.join(","),
        }
    }

    pub fn patterns(&self) -> Vec<String> {
        match self {
            GlobPattern::Single(s) => vec![s.clone()],
            GlobPattern::Multiple(v) => v.clone(),
        }
    }

    fn from_yaml(node: &Yaml) -> Option<Self> {
        if let Some(s) = node.as_str() {
            Some(GlobPattern::Single(s.to_string()))
        } else if let Some(v) = node.as_vec() {
            let patterns = v
                .iter()
                .filter_map(|n| n.as_str().map(String::from))
                .collect();
            Some(GlobPattern::Multiple(patterns))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProjectConfig {
    schema: SchemaSource,
    include: GlobPattern,
    exclude: Option<GlobPattern>,
    output_dir: Option<String>,
    import: Option<String>,
    codegen_enabled: Option<bool>,
    codegen: Option<CodegenConfig>,
    possible_types: Option<PathBuf>,
    type_policies: Option<PathBuf>,
}

impl ProjectConfig {
    pub fn with_schema(mut self, schema: SchemaSource) -> Self {
        self.schema = schema;
        self
    }

    pub fn with_include(mut self, include: GlobPattern) -> Self {
        self.include = include;
        self
    }

    pub fn with_exclude(mut self, exclude: GlobPattern) -> Self {
        self.exclude = Some(exclude);
        self
    }

    pub fn with_output_dir(mut self, output_dir: String) -> Self {
        self.output_dir = Some(output_dir);
        self
    }

    pub fn with_import(mut self, import: String) -> Self {
        self.import = Some(import);
        self
    }

    pub fn with_codegen(mut self, codegen: CodegenConfig) -> Self {
        self.codegen = Some(codegen);
        self
    }

    pub fn schema(&self) -> &SchemaSource {
        &self.schema
    }

    pub fn include(&self) -> &GlobPattern {
        &self.include
    }

    pub fn exclude(&self) -> Option<&GlobPattern> {
        self.exclude.as_ref()
    }

    pub fn output_dir(&self) -> Option<&str> {
        self.output_dir.as_deref()
    }

    pub fn import(&self) -> Option<&str> {
        self.import.as_deref()
    }

    pub fn codegen(&self) -> &CodegenConfig {
        static DEFAULT: LazyLock<CodegenConfig> = LazyLock::new(CodegenConfig::default);
        self.codegen.as_ref().unwrap_or(&DEFAULT)
    }

    pub fn possible_types(&self) -> Option<&Path> {
        self.possible_types.as_deref()
    }

    pub fn type_policies(&self) -> Option<&Path> {
        self.type_policies.as_deref()
    }
}

#[derive(Debug, Clone)]
pub struct SchemaTypeConfig {
    schema: SchemaSource,
    output: String,
    import: Option<String>,
    possible_types: Option<PathBuf>,
    type_policies: Option<PathBuf>,
}

impl SchemaTypeConfig {
    pub fn schema(&self) -> &SchemaSource {
        &self.schema
    }

    pub fn output(&self) -> &str {
        &self.output
    }

    pub fn import(&self) -> Option<&str> {
        self.import.as_deref()
    }

    pub fn possible_types(&self) -> Option<&Path> {
        self.possible_types.as_deref()
    }

    pub fn type_policies(&self) -> Option<&Path> {
        self.type_policies.as_deref()
    }
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    projects: Vec<ProjectConfig>,
    schema_types: Option<Vec<SchemaTypeConfig>>,
    scalars: Option<AHashMap<String, String>>,
    ignore_deprecations: Option<Vec<String>>,
    tracing: Option<TracingConfig>,
    timeouts: Option<TimeoutConfig>,
    watch_all_files: Option<bool>,
    lsp_automatic_codegen: Option<bool>,
    lsp_codegen_throttle_ms: Option<u64>,
    codegen_watch_debounce_ms: Option<u64>,
    enable_schema_cache: Option<bool>,
    rules: Option<RulesConfig>,
    codegen: Option<CodegenConfig>,
    base_dir: PathBuf,
}

impl Config {
    pub fn with_base_dir(mut self, base_dir: PathBuf) -> Self {
        self.base_dir = base_dir;
        self
    }

    pub fn with_projects(mut self, projects: Vec<ProjectConfig>) -> Self {
        self.projects = projects;
        self
    }

    pub fn with_enable_schema_cache(mut self, enabled: bool) -> Self {
        self.enable_schema_cache = Some(enabled);
        self
    }

    pub fn with_lsp_automatic_codegen(mut self, enabled: bool) -> Self {
        self.lsp_automatic_codegen = Some(enabled);
        self
    }

    pub fn with_lsp_codegen_throttle_ms(mut self, ms: u64) -> Self {
        self.lsp_codegen_throttle_ms = Some(ms);
        self
    }

    pub fn with_watch_all_files(mut self, enabled: bool) -> Self {
        self.watch_all_files = Some(enabled);
        self
    }

    pub fn with_timeouts(mut self, timeouts: TimeoutConfig) -> Self {
        self.timeouts = Some(timeouts);
        self
    }

    pub fn with_rules(mut self, rules: RulesConfig) -> Self {
        self.rules = Some(rules);
        self
    }

    pub fn projects(&self) -> &[ProjectConfig] {
        &self.projects
    }

    pub fn schema_types(&self) -> &[SchemaTypeConfig] {
        self.schema_types.as_deref().unwrap_or(&[])
    }

    pub fn scalars(&self) -> &AHashMap<String, String> {
        static EMPTY: LazyLock<AHashMap<String, String>> = LazyLock::new(AHashMap::default);
        self.scalars.as_ref().unwrap_or(&EMPTY)
    }

    pub fn ignore_deprecations(&self) -> &[String] {
        self.ignore_deprecations.as_deref().unwrap_or(&[])
    }

    pub fn tracing(&self) -> TracingConfig {
        self.tracing.clone().unwrap_or(TracingConfig {
            enabled: false,
            threshold_ms: 20,
        })
    }

    pub fn rules(&self) -> RulesConfig {
        self.rules.clone().unwrap_or_default()
    }

    pub fn codegen(&self) -> CodegenConfig {
        self.codegen.clone().unwrap_or_default()
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn get_codegen_config(&self, project: Option<&ProjectConfig>) -> CodegenConfig {
        let mut result = self.codegen();

        if let Some(project) = project {
            let project_codegen = project.codegen();
            if project_codegen.document_suffix.is_some() {
                result.document_suffix = project_codegen.document_suffix.clone();
            }
            if project_codegen.variables_suffix.is_some() {
                result.variables_suffix = project_codegen.variables_suffix.clone();
            }
            if project_codegen.fragment_suffix.is_some() {
                result.fragment_suffix = project_codegen.fragment_suffix.clone();
            }
            if project_codegen.fragment_document_suffix.is_some() {
                result.fragment_document_suffix = project_codegen.fragment_document_suffix.clone();
            }
            if project_codegen.query_suffix.is_some() {
                result.query_suffix = project_codegen.query_suffix.clone();
            }
            if project_codegen.mutation_suffix.is_some() {
                result.mutation_suffix = project_codegen.mutation_suffix.clone();
            }
            if project_codegen.subscription_suffix.is_some() {
                result.subscription_suffix = project_codegen.subscription_suffix.clone();
            }
            if project_codegen.naming_convention.is_some() {
                result.naming_convention = project_codegen.naming_convention.clone();
            }
            if project_codegen.fragment_masking.is_some() {
                result.fragment_masking = project_codegen.fragment_masking.clone();
            }
            if project_codegen.emit_extensions.is_some() {
                result.emit_extensions = project_codegen.emit_extensions;
            }
            if project_codegen.generate_ast_for_fragments.is_some() {
                result.generate_ast_for_fragments = project_codegen.generate_ast_for_fragments;
            }
            if project_codegen.re_exports.is_some() {
                result.re_exports = project_codegen.re_exports;
            }
            if project_codegen.emit_permission_data.is_some() {
                result.emit_permission_data = project_codegen.emit_permission_data;
            }
            if project_codegen.emit_ast_directives.is_some() {
                result.emit_ast_directives = project_codegen.emit_ast_directives;
            }
            if project_codegen.emit_ast_aliases.is_some() {
                result.emit_ast_aliases = project_codegen.emit_ast_aliases;
            }
            if project_codegen.emit_ast_arguments.is_some() {
                result.emit_ast_arguments = project_codegen.emit_ast_arguments;
            }
            if project_codegen.emit_ast_variable_defaults.is_some() {
                result.emit_ast_variable_defaults = project_codegen.emit_ast_variable_defaults;
            }
            if project_codegen.inline_fragments.is_some() {
                result.inline_fragments = project_codegen.inline_fragments;
            }
            if project_codegen.default_scalar_type.is_some() {
                result.default_scalar_type = project_codegen.default_scalar_type.clone();
            }
            if project_codegen.schema_import.is_some() {
                result.schema_import = project_codegen.schema_import.clone();
            }
        }

        result
    }

    pub fn get_project_codegen_enabled(&self, project: &ProjectConfig) -> bool {
        project.codegen_enabled.unwrap_or_else(|| {
            project
                .codegen
                .as_ref()
                .and_then(|c| c.enabled)
                .unwrap_or_else(|| self.codegen().is_enabled())
        })
    }

    pub fn get_emit_extensions(&self, project: &ProjectConfig) -> EmitExtensions {
        self.get_codegen_config(Some(project)).emit_extensions()
    }

    pub fn get_project_for_path(&self, path: &Path) -> Option<&ProjectConfig> {
        let abs_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let relative_path = abs_path.strip_prefix(&self.base_dir).ok();

        for project in &self.projects {
            let mut matched = false;
            if let Some(rel_path) = relative_path {
                let include_set = get_glob_set(&project.include().patterns());
                if include_set.is_match(rel_path) {
                    matched = true;
                } else {
                    // If it didn't match as a glob, check if it's a sub-path of any of the include patterns
                    // that are not globs themselves.
                    for pattern in project.include().patterns() {
                        if !pattern.contains('*')
                            && !pattern.contains('?')
                            && !pattern.contains('[')
                            && !pattern.contains('{')
                            && rel_path.starts_with(Path::new(&pattern))
                        {
                            matched = true;
                            break;
                        }
                    }
                }
            }

            if !matched {
                for pattern in project.include().patterns() {
                    let include_path = self.base_dir.join(&pattern);
                    if let Ok(include_path) = fs::canonicalize(include_path)
                        && crate::utils::path_starts_with(&abs_path, &include_path)
                    {
                        matched = true;
                        break;
                    }
                }
            }

            if matched
                && let Some(exclude) = project.exclude()
                && let Some(rel_path) = relative_path
            {
                let exclude_set = get_glob_set(&exclude.patterns());
                if exclude_set.is_match(rel_path) {
                    matched = false;
                }
            }

            if matched {
                return Some(project);
            }
        }
        None
    }

    pub fn get_schema_for_path(&self, path: &Path) -> Option<String> {
        self.get_project_for_path(path).map(|p| p.schema().as_key())
    }

    pub fn watch_all_files(&self) -> bool {
        self.watch_all_files.unwrap_or(true)
    }

    pub fn lsp_automatic_codegen(&self) -> bool {
        self.codegen().is_enabled() && self.lsp_automatic_codegen.unwrap_or(true)
    }

    pub fn lsp_codegen_throttle_ms(&self) -> u64 {
        self.lsp_codegen_throttle_ms.unwrap_or(300)
    }

    pub fn codegen_watch_debounce_ms(&self) -> u64 {
        self.codegen_watch_debounce_ms.unwrap_or(200)
    }

    pub fn enable_schema_cache(&self) -> bool {
        self.enable_schema_cache.unwrap_or(true)
    }

    pub fn get_timeouts(&self) -> TimeoutConfig {
        self.timeouts.clone().unwrap_or_default()
    }

    pub fn document_suffix(&self) -> &str {
        static DEFAULT: &str = "Document";
        self.codegen
            .as_ref()
            .and_then(|c| c.document_suffix.as_deref())
            .unwrap_or(DEFAULT)
    }

    pub fn variables_suffix(&self) -> &str {
        static DEFAULT: &str = "Variables";
        self.codegen
            .as_ref()
            .and_then(|c| c.variables_suffix.as_deref())
            .unwrap_or(DEFAULT)
    }

    pub fn fragment_suffix(&self) -> &str {
        static DEFAULT: &str = "";
        self.codegen
            .as_ref()
            .and_then(|c| c.fragment_suffix.as_deref())
            .unwrap_or(DEFAULT)
    }

    pub fn fragment_document_suffix(&self) -> &str {
        static DEFAULT: &str = "";
        self.codegen
            .as_ref()
            .and_then(|c| c.fragment_document_suffix.as_deref())
            .unwrap_or(DEFAULT)
    }

    pub fn query_suffix(&self) -> &str {
        static DEFAULT: &str = "Query";
        self.codegen
            .as_ref()
            .and_then(|c| c.query_suffix.as_deref())
            .unwrap_or(DEFAULT)
    }

    pub fn mutation_suffix(&self) -> &str {
        static DEFAULT: &str = "Mutation";
        self.codegen
            .as_ref()
            .and_then(|c| c.mutation_suffix.as_deref())
            .unwrap_or(DEFAULT)
    }

    pub fn subscription_suffix(&self) -> &str {
        static DEFAULT: &str = "Subscription";
        self.codegen
            .as_ref()
            .and_then(|c| c.subscription_suffix.as_deref())
            .unwrap_or(DEFAULT)
    }

    pub fn naming_convention(&self) -> NamingConvention {
        self.codegen
            .as_ref()
            .and_then(|c| c.naming_convention.clone())
            .unwrap_or_default()
    }

    pub fn new_empty() -> Self {
        Self {
            projects: vec![],
            schema_types: None,
            scalars: None,
            ignore_deprecations: None,
            tracing: None,
            timeouts: None,
            watch_all_files: None,
            lsp_automatic_codegen: None,
            lsp_codegen_throttle_ms: None,
            codegen_watch_debounce_ms: None,
            enable_schema_cache: None,
            rules: None,
            codegen: None,
            base_dir: PathBuf::from("."),
        }
    }

    pub fn new_test(base_dir: PathBuf, projects: Vec<ProjectConfig>) -> Self {
        Self {
            base_dir: fs::canonicalize(&base_dir).unwrap_or(base_dir),
            projects,
            ..Self::new_empty()
        }
    }

    pub fn load() -> Self {
        let mut curr = std::env::current_dir().unwrap_or_else(|e| {
            eprintln!(
                "{}: Failed to get current directory: {}",
                "Error".red(),
                e.to_string().red()
            );
            std::process::exit(1);
        });
        loop {
            match Self::load_from_dir(&curr) {
                Ok(Some(config)) => return config,
                Ok(None) => {
                    if let Some(parent) = curr.parent() {
                        curr = parent.to_path_buf();
                    } else {
                        eprintln!(
                            "{}: No graphox.yaml or graphox.yml found in current or parent directories. This tool requires a configuration file to run.",
                            "Error".red()
                        );
                        std::process::exit(1);
                    }
                }
                Err((path, error)) => {
                    eprintln!(
                        "{}: Failed to parse {}: {}",
                        "Error".red(),
                        path.display().to_string().red(),
                        error.red()
                    );
                    std::process::exit(1);
                }
            }
        }
    }

    pub fn load_from_dir<P: AsRef<Path>>(dir: P) -> Result<Option<Self>, (PathBuf, String)> {
        let dir = dir.as_ref();
        let yaml_path = dir.join("graphox.yaml");
        let yml_path = dir.join("graphox.yml");

        let config_path = if yaml_path.exists() {
            yaml_path
        } else if yml_path.exists() {
            yml_path
        } else {
            return Ok(None);
        };

        let content =
            fs::read_to_string(&config_path).map_err(|e| (config_path.clone(), e.to_string()))?;
        let docs = yaml_rust2::YamlLoader::load_from_str(&content)
            .map_err(|e| (config_path.clone(), format!("{:?}", e)))?;
        let doc = docs
            .first()
            .ok_or_else(|| (config_path.clone(), "Empty YAML document".to_string()))?;

        let mut config = Config::from_yaml(doc).ok_or_else(|| {
            (
                config_path.clone(),
                "Invalid configuration format".to_string(),
            )
        })?;
        config.base_dir = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        Ok(Some(config))
    }

    fn from_yaml(node: &Yaml) -> Option<Self> {
        let mut config = Config::new_empty();

        config.codegen = CodegenConfig::from_yaml(&node["codegen"]);

        if let Some(projects_node) = node["projects"].as_vec() {
            for p_node in projects_node {
                let schema = SchemaSource::from_yaml(&p_node["schema"])?;
                let include = GlobPattern::from_yaml(&p_node["include"])
                    .or_else(|| GlobPattern::from_yaml(&p_node["documents"]))?;
                let exclude = GlobPattern::from_yaml(&p_node["exclude"]);
                let output_dir = p_node["output_dir"].as_str().map(String::from);
                let import = p_node["import"].as_str().map(String::from);
                let codegen_enabled = p_node["codegen"].as_bool();
                let codegen_config = CodegenConfig::from_yaml(&p_node["codegen"]);
                let possible_types = p_node["possible_types"].as_str().map(PathBuf::from);
                let type_policies = p_node["type_policies"].as_str().map(PathBuf::from);

                config.projects.push(ProjectConfig {
                    schema,
                    include,
                    exclude,
                    output_dir,
                    import,
                    codegen_enabled,
                    codegen: codegen_config,
                    possible_types,
                    type_policies,
                });
            }
        }

        if let Some(st_node) = node["schema_types"].as_vec() {
            let mut schema_types = Vec::new();
            for s_node in st_node {
                let schema = SchemaSource::from_yaml(&s_node["schema"])?;
                let output = s_node["output"].as_str()?.to_string();
                let import = s_node["import"].as_str().map(String::from);
                let possible_types = s_node["possible_types"].as_str().map(PathBuf::from);
                let type_policies = s_node["type_policies"].as_str().map(PathBuf::from);
                schema_types.push(SchemaTypeConfig {
                    schema,
                    output,
                    import,
                    possible_types,
                    type_policies,
                });
            }
            config.schema_types = Some(schema_types);
        }

        if let Some(scalars_hash) = node["scalars"].as_hash() {
            let mut scalars = AHashMap::default();
            for (k, v) in scalars_hash {
                if let (Some(k), Some(v)) = (k.as_str(), v.as_str()) {
                    scalars.insert(k.to_string(), v.to_string());
                }
            }
            config.scalars = Some(scalars);
        }

        config.ignore_deprecations = node["ignore_deprecations"].as_vec().map(|v| {
            v.iter()
                .filter_map(|n| n.as_str().map(String::from))
                .collect()
        });

        let tracing_node = &node["tracing"];
        if !tracing_node.is_badvalue() && !tracing_node.is_null() {
            config.tracing = Some(TracingConfig {
                enabled: tracing_node["enabled"].as_bool().unwrap_or(false),
                threshold_ms: tracing_node["threshold_ms"]
                    .as_i64()
                    .map(|v| v as u64)
                    .unwrap_or(20),
            });
        }

        let timeouts_node = &node["timeouts"];
        if !timeouts_node.is_badvalue() && !timeouts_node.is_null() {
            config.timeouts = Some(TimeoutConfig {
                workspace_scan_ms: timeouts_node["workspace_scan_ms"]
                    .as_i64()
                    .map(|v| v as u64)
                    .unwrap_or(60_000),
                lsp_request_ms: timeouts_node["lsp_request_ms"]
                    .as_i64()
                    .map(|v| v as u64)
                    .unwrap_or(1_000),
            });
        }

        config.watch_all_files = node["watch_all_files"].as_bool();
        config.lsp_automatic_codegen = node["lsp_automatic_codegen"].as_bool();
        config.lsp_codegen_throttle_ms = node["lsp_codegen_throttle_ms"].as_i64().map(|v| v as u64);
        config.codegen_watch_debounce_ms =
            node["codegen_watch_debounce_ms"].as_i64().map(|v| v as u64);
        config.enable_schema_cache = node["enable_schema_cache"].as_bool();

        let rules_node = &node["rules"];
        if !rules_node.is_badvalue() && !rules_node.is_null() {
            let mut rules = RulesConfig::default();
            if let Some(rf_hash) = rules_node["required_fields"].as_hash() {
                let mut required_fields = AHashMap::default();
                for (k, v) in rf_hash {
                    if let (Some(k), Some(rule)) = (k.as_str(), RequiredFieldRule::from_yaml(v)) {
                        required_fields.insert(k.to_string(), rule);
                    }
                }
                rules.required_fields = Some(required_fields);
            }
            rules.unique_operation_name = rules_node["unique_operation_name"].as_bool();
            rules.no_duplicate_fields = rules_node["no_duplicate_fields"].as_bool();
            rules.no_unused_fragments = rules_node["no_unused_fragments"].as_bool();
            config.rules = Some(rules);
        }

        config.codegen = CodegenConfig::from_yaml(&node["codegen"]);

        Some(config)
    }
}
