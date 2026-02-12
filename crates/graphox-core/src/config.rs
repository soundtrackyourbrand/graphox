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
    pub required_fields: Option<AHashMap<String, RequiredFieldRule>>,
    pub unique_operation_name: Option<bool>,
    pub no_duplicate_fields: Option<bool>,
    pub no_unused_fragments: Option<bool>,
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
            Some(RequiredFieldRule::Always(b))
        } else if let Some(vec) = node.as_vec() {
            let ops = vec
                .iter()
                .filter_map(|n| n.as_str().map(String::from))
                .collect();
            Some(RequiredFieldRule::Operations(ops))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum EmitExtensions {
    #[default]
    None,
    Js,
    Ts,
}

impl EmitExtensions {
    pub fn from_yaml(node: &Yaml) -> Self {
        if let Some(s) = node.as_str() {
            match s.to_lowercase().as_str() {
                "js" => EmitExtensions::Js,
                "ts" => EmitExtensions::Ts,
                _ => EmitExtensions::None,
            }
        } else {
            EmitExtensions::None
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            EmitExtensions::None => "",
            EmitExtensions::Js => ".js",
            EmitExtensions::Ts => ".ts",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub projects: Vec<ProjectConfig>,
    pub schema_types: Option<Vec<SchemaTypeConfig>>,
    pub scalars: Option<AHashMap<String, String>>,
    pub ignore_deprecations: Option<Vec<String>>,
    pub generate_ast_for_fragments: Option<bool>,
    pub tracing: Option<TracingConfig>,
    pub timeouts: Option<TimeoutConfig>,
    pub watch_all_files: Option<bool>,
    pub lsp_automatic_codegen: Option<bool>,
    pub lsp_codegen_throttle_ms: Option<u64>,
    pub codegen_watch_debounce_ms: Option<u64>,
    pub enable_schema_cache: Option<bool>,
    pub rules: Option<RulesConfig>,
    pub document_suffix: Option<String>,
    pub variables_suffix: Option<String>,
    pub fragment_suffix: Option<String>,
    pub fragment_document_suffix: Option<String>,
    pub query_suffix: Option<String>,
    pub mutation_suffix: Option<String>,
    pub subscription_suffix: Option<String>,
    pub fragment_masking: Option<FragmentMaskingConfig>,
    pub emit_extensions: Option<EmitExtensions>,
    pub base_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct TracingConfig {
    pub enabled: bool,
    pub threshold_ms: u64,
}

#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    pub workspace_scan_ms: u64,
    pub lsp_request_ms: u64,
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

#[derive(Debug, Clone, Default)]
pub enum FragmentMasking {
    #[default]
    Disabled,
    Enabled {
        unmask_function_name: Option<String>,
    },
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

#[derive(Debug, Clone)]
pub struct FragmentMaskingConfig {
    pub mode: FragmentMasking,
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
    pub schema: SchemaSource,
    pub include: GlobPattern,
    pub exclude: Option<GlobPattern>,
    pub output_dir: Option<String>,
    pub import: Option<String>,
    pub emit_permission_data: Option<bool>,
    pub codegen: Option<bool>,
    pub document_suffix: Option<String>,
    pub variables_suffix: Option<String>,
    pub fragment_suffix: Option<String>,
    pub fragment_document_suffix: Option<String>,
    pub query_suffix: Option<String>,
    pub mutation_suffix: Option<String>,
    pub subscription_suffix: Option<String>,
    pub fragment_masking: Option<FragmentMaskingConfig>,
    pub emit_extensions: Option<EmitExtensions>,
    pub possible_types: Option<PathBuf>,
    pub type_policies: Option<PathBuf>,
    pub generate_ast_for_fragments: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct SchemaTypeConfig {
    pub schema: SchemaSource,
    pub output: String,
    pub import: Option<String>,
    pub possible_types: Option<PathBuf>,
    pub type_policies: Option<PathBuf>,
}

impl Config {
    pub fn new_empty() -> Self {
        Self {
            projects: vec![],
            schema_types: None,
            scalars: None,
            ignore_deprecations: None,
            generate_ast_for_fragments: None,
            tracing: None,
            timeouts: None,
            watch_all_files: None,
            lsp_automatic_codegen: None,
            lsp_codegen_throttle_ms: None,
            codegen_watch_debounce_ms: None,
            enable_schema_cache: None,
            rules: None,
            document_suffix: None,
            variables_suffix: None,
            fragment_suffix: None,
            fragment_document_suffix: None,
            query_suffix: None,
            mutation_suffix: None,
            subscription_suffix: None,
            fragment_masking: None,
            emit_extensions: None,
            base_dir: PathBuf::from("."),
        }
    }

    #[cfg(test)]
    pub fn new_test(base_dir: PathBuf, projects: Vec<ProjectConfig>) -> Self {
        Self {
            base_dir,
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

        if let Some(projects_node) = node["projects"].as_vec() {
            for p_node in projects_node {
                let schema = SchemaSource::from_yaml(&p_node["schema"])?;
                let include = GlobPattern::from_yaml(&p_node["include"])?;
                let exclude = GlobPattern::from_yaml(&p_node["exclude"]);
                let output_dir = p_node["output_dir"].as_str().map(String::from);
                let import = p_node["import"].as_str().map(String::from);
                let emit_permission_data = p_node["emit_permission_data"].as_bool();
                let codegen = p_node["codegen"].as_bool();
                let document_suffix = p_node["document_suffix"].as_str().map(String::from);
                let variables_suffix = p_node["variables_suffix"].as_str().map(String::from);
                let fragment_suffix = p_node["fragment_suffix"].as_str().map(String::from);
                let fragment_document_suffix = p_node["fragment_document_suffix"]
                    .as_str()
                    .map(String::from);
                let query_suffix = p_node["query_suffix"].as_str().map(String::from);
                let mutation_suffix = p_node["mutation_suffix"].as_str().map(String::from);
                let subscription_suffix = p_node["subscription_suffix"].as_str().map(String::from);
                let fragment_masking_node = &p_node["fragment_masking"];
                let fragment_masking = FragmentMasking::from_yaml(fragment_masking_node)
                    .map(|mode| FragmentMaskingConfig { mode });
                let emit_extensions = Some(EmitExtensions::from_yaml(&p_node["emit_extensions"]));
                let possible_types = p_node["possible_types"].as_str().map(PathBuf::from);
                let type_policies = p_node["type_policies"].as_str().map(PathBuf::from);
                let generate_ast_for_fragments = p_node["generate_ast_for_fragments"].as_bool();

                config.projects.push(ProjectConfig {
                    schema,
                    include,
                    exclude,
                    output_dir,
                    import,
                    emit_permission_data,
                    codegen,
                    document_suffix,
                    variables_suffix,
                    fragment_suffix,
                    fragment_document_suffix,
                    query_suffix,
                    mutation_suffix,
                    subscription_suffix,
                    fragment_masking,
                    emit_extensions,
                    possible_types,
                    type_policies,
                    generate_ast_for_fragments,
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

        config.generate_ast_for_fragments = node["generate_ast_for_fragments"].as_bool();

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

        config.document_suffix = node["document_suffix"].as_str().map(String::from);
        config.variables_suffix = node["variables_suffix"].as_str().map(String::from);
        config.fragment_suffix = node["fragment_suffix"].as_str().map(String::from);
        config.fragment_document_suffix =
            node["fragment_document_suffix"].as_str().map(String::from);
        config.query_suffix = node["query_suffix"].as_str().map(String::from);
        config.mutation_suffix = node["mutation_suffix"].as_str().map(String::from);
        config.subscription_suffix = node["subscription_suffix"].as_str().map(String::from);

        let fragment_masking_node = &node["fragment_masking"];
        if let Some(mode) = FragmentMasking::from_yaml(fragment_masking_node) {
            config.fragment_masking = Some(FragmentMaskingConfig { mode });
        }

        config.emit_extensions = Some(EmitExtensions::from_yaml(&node["emit_extensions"]));

        Some(config)
    }

    pub fn get_emit_extensions(&self, project: &ProjectConfig) -> EmitExtensions {
        project
            .emit_extensions
            .or(self.emit_extensions)
            .unwrap_or(EmitExtensions::None)
    }

    pub fn get_project_for_path(&self, path: &Path) -> Option<&ProjectConfig> {
        let abs_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let relative_path = abs_path.strip_prefix(&self.base_dir).ok();

        for project in &self.projects {
            let mut matched = false;
            if let Some(rel_path) = relative_path {
                let include_set = get_glob_set(&project.include.patterns());
                if include_set.is_match(rel_path) {
                    matched = true;
                }
            }

            if !matched {
                for pattern in project.include.patterns() {
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
                && let Some(exclude) = &project.exclude
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
        self.get_project_for_path(path).map(|p| p.schema.as_key())
    }

    pub fn watch_all_files(&self) -> bool {
        self.watch_all_files.unwrap_or(true)
    }

    pub fn lsp_automatic_codegen(&self) -> bool {
        self.lsp_automatic_codegen.unwrap_or(true)
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
        self.document_suffix.as_deref().unwrap_or("Document")
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
            .or(self.fragment_suffix.as_deref())
            .unwrap_or("")
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

    pub fn fragment_masking(&self) -> FragmentMaskingConfig {
        FragmentMaskingConfig {
            mode: self
                .fragment_masking
                .as_ref()
                .map(|c| c.mode.clone())
                .unwrap_or_default(),
        }
    }

    pub fn fragment_masking_mode(&self) -> FragmentMasking {
        self.fragment_masking
            .as_ref()
            .map(|c| c.mode.clone())
            .unwrap_or_default()
    }
}

impl FragmentMaskingConfig {
    pub fn mode(&self) -> FragmentMasking {
        self.mode.clone()
    }
}

impl Default for FragmentMaskingConfig {
    fn default() -> Self {
        Self {
            mode: FragmentMasking::Disabled,
        }
    }
}

impl ProjectConfig {
    pub fn codegen_enabled(&self) -> bool {
        self.codegen.unwrap_or(true)
    }

    pub fn fragment_masking_mode(&self) -> Option<FragmentMasking> {
        self.fragment_masking.as_ref().map(|c| c.mode.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    #[ntest::timeout(100)]
    fn test_load_yaml() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("graphox.yaml");
        let mut file = fs::File::create(config_path).unwrap();
        writeln!(
            file,
            r#"
projects:
  - schema: "s1.graphql"
    include: "src/p1/**/*.ts"
  - schema: "s2.graphql"
    include: "src/p2/**/*.ts"
    output_dir: "gen2"
 "#
        )
        .unwrap();

        let config = Config::load_from_dir(dir.path()).unwrap().unwrap();
        assert_eq!(config.projects.len(), 2);
        assert_eq!(config.projects[0].schema.as_key(), "s1.graphql");
        assert_eq!(config.projects[1].output_dir, Some("gen2".to_string()));
    }

    #[test]
    #[ntest::timeout(100)]
    fn test_load_yml() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("graphox.yml");
        let mut file = fs::File::create(config_path).unwrap();
        writeln!(
            file,
            r#"
projects:
  - schema: "s.graphql"
    include: "src/**/*.ts"
"#
        )
        .unwrap();

        let config = Config::load_from_dir(dir.path()).unwrap().unwrap();
        assert_eq!(config.projects.len(), 1);
        assert_eq!(config.projects[0].schema.as_key(), "s.graphql");
    }

    #[test]
    #[ntest::timeout(100)]
    fn test_load_parent_dir() {
        let dir = tempdir().unwrap();
        let parent_dir = dir.path().join("parent");
        let child_dir = parent_dir.join("child");
        fs::create_dir_all(&child_dir).unwrap();

        let config_path = parent_dir.join("graphox.yaml");
        let mut file = fs::File::create(config_path).unwrap();
        writeln!(
            file,
            r#"
projects:
  - schema: "s.graphql"
    include: "**/*.ts"
"#
        )
        .unwrap();

        // Change current directory to child_dir to test upward search
        let original_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&child_dir).unwrap();

        let config = Config::load();

        // Restore original directory
        std::env::set_current_dir(original_dir).unwrap();

        assert_eq!(config.projects.len(), 1);
        assert_eq!(config.projects[0].schema.as_key(), "s.graphql");

        // Test that paths are resolved relative to the config file
        let file_in_child = child_dir.join("test.ts");
        fs::File::create(&file_in_child).unwrap();
        assert_eq!(
            config.get_schema_for_path(&file_in_child),
            Some("s.graphql".to_string())
        );
    }

    #[test]
    #[ntest::timeout(100)]
    fn test_include_exclude() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("graphox.yaml");
        let mut file = fs::File::create(config_path).unwrap();
        writeln!(
            file,
            r#"
projects:
  - schema: "s.graphql"
    include: 
      - "src/**/*.ts"
      - "lib/**/*.ts"
    exclude: "**/test.ts"
"#
        )
        .unwrap();

        let config = Config::load_from_dir(dir.path()).unwrap().unwrap();
        assert_eq!(config.projects.len(), 1);
        let project = &config.projects[0];
        assert_eq!(project.include.patterns().len(), 2);
        assert_eq!(project.exclude.as_ref().unwrap().patterns().len(), 1);

        let ts_file = dir.path().join("src/main.ts");
        let test_file = dir.path().join("src/test.ts");
        let lib_file = dir.path().join("lib/index.ts");
        let other_file = dir.path().join("other/file.ts");

        fs::create_dir_all(ts_file.parent().unwrap()).unwrap();
        fs::create_dir_all(lib_file.parent().unwrap()).unwrap();
        fs::create_dir_all(other_file.parent().unwrap()).unwrap();
        fs::File::create(&ts_file).unwrap();
        fs::File::create(&test_file).unwrap();
        fs::File::create(&lib_file).unwrap();
        fs::File::create(&other_file).unwrap();

        // Canonicalize base dir for matching
        let config = Config::load_from_dir(fs::canonicalize(dir.path()).unwrap())
            .unwrap()
            .unwrap();

        assert_eq!(
            config.get_schema_for_path(&ts_file),
            Some("s.graphql".to_string())
        );
        assert_eq!(config.get_schema_for_path(&test_file), None);
        assert_eq!(
            config.get_schema_for_path(&lib_file),
            Some("s.graphql".to_string())
        );
        assert_eq!(config.get_schema_for_path(&other_file), None);
    }

    #[test]
    #[ntest::timeout(100)]
    fn test_codegen_disabled() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("graphox.yaml");
        let mut file = fs::File::create(config_path).unwrap();
        writeln!(
            file,
            r#"
projects:
  - schema: "s1.graphql"
    include: "src/p1/**/*.ts"
    codegen: false
  - schema: "s2.graphql"
    include: "src/p2/**/*.ts"
  - schema: "s3.graphql"
    include: "src/p3/**/*.ts"
    codegen: true
"#
        )
        .unwrap();

        let config = Config::load_from_dir(dir.path()).unwrap().unwrap();
        assert_eq!(config.projects.len(), 3);

        // First project has codegen disabled
        assert!(!config.projects[0].codegen_enabled());

        // Second project has default (enabled)
        assert!(config.projects[1].codegen_enabled());

        // Third project has codegen explicitly enabled
        assert!(config.projects[2].codegen_enabled());
    }

    #[test]
    #[ntest::timeout(100)]
    fn test_fragment_masking_disabled() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("graphox.yaml");
        let mut file = fs::File::create(config_path).unwrap();
        writeln!(
            file,
            r#"
fragment_masking: disabled
projects:
  - schema: "s.graphql"
    include: "src/**/*.ts"
"#
        )
        .unwrap();

        let config = Config::load_from_dir(dir.path()).unwrap().unwrap();
        assert!(matches!(
            config.fragment_masking_mode(),
            FragmentMasking::Disabled
        ));
    }

    #[test]
    #[ntest::timeout(100)]
    fn test_fragment_masking_enabled() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("graphox.yaml");
        let mut file = fs::File::create(config_path).unwrap();
        writeln!(
            file,
            r#"
fragment_masking: enabled
projects:
  - schema: "s.graphql"
    include: "src/**/*.ts"
"#
        )
        .unwrap();

        let config = Config::load_from_dir(dir.path()).unwrap().unwrap();
        assert!(matches!(
            config.fragment_masking_mode(),
            FragmentMasking::Enabled { .. }
        ));
    }

    #[test]
    #[ntest::timeout(100)]
    fn test_fragment_masking_enabled_with_custom_function() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("graphox.yaml");
        let mut file = fs::File::create(config_path).unwrap();
        writeln!(
            file,
            r#"
fragment_masking:
  unmask_function_name: getData
projects:
  - schema: "s.graphql"
    include: "src/**/*.ts"
"#
        )
        .unwrap();

        let config = Config::load_from_dir(dir.path()).unwrap().unwrap();
        match config.fragment_masking_mode() {
            FragmentMasking::Enabled {
                unmask_function_name,
            } => {
                assert_eq!(unmask_function_name.as_deref(), Some("getData"));
            }
            _ => panic!("Expected FragmentMasking::Enabled"),
        }
    }

    #[test]
    #[ntest::timeout(100)]
    fn test_fragment_masking_project_override() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("graphox.yaml");
        let mut file = fs::File::create(config_path).unwrap();
        writeln!(
            file,
            r#"
fragment_masking: enabled
projects:
  - schema: "s1.graphql"
    include: "src/p1/**/*.ts"
  - schema: "s2.graphql"
    include: "src/p2/**/*.ts"
    fragment_masking: disabled
"#
        )
        .unwrap();

        let config = Config::load_from_dir(dir.path()).unwrap().unwrap();
        assert!(matches!(
            config.fragment_masking_mode(),
            FragmentMasking::Enabled { .. }
        ));
        assert!(matches!(
            config.projects[1].fragment_masking_mode(),
            Some(FragmentMasking::Disabled)
        ));
    }
}
