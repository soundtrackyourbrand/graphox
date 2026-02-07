use colored::*;
use fnv::FnvHashMap;
use globset::{Glob, GlobSetBuilder};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone, Default)]
pub struct RulesConfig {
    pub required_fields: Option<FnvHashMap<String, RequiredFieldRule>>,
    pub unique_operation_name: Option<bool>,
    pub no_duplicate_fields: Option<bool>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
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
}

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub output_dir: Option<String>,
    pub projects: Vec<ProjectConfig>,
    pub schema_types: Option<Vec<SchemaTypeConfig>>,
    pub scalars: Option<FnvHashMap<String, String>>,
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
    #[serde(skip)]
    pub base_dir: PathBuf,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TracingConfig {
    pub enabled: bool,
    #[serde(default = "default_threshold")]
    pub threshold_ms: u64,
}

fn default_threshold() -> u64 {
    20
}

#[derive(Debug, Deserialize, Clone)]
pub struct TimeoutConfig {
    #[serde(default = "default_workspace_scan_timeout_ms")]
    pub workspace_scan_ms: u64,
    #[serde(default = "default_lsp_request_timeout_ms")]
    pub lsp_request_ms: u64,
}

fn default_workspace_scan_timeout_ms() -> u64 {
    60_000 // 1 minute
}

fn default_lsp_request_timeout_ms() -> u64 {
    1_000 // 1 second
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            workspace_scan_ms: default_workspace_scan_timeout_ms(),
            lsp_request_ms: default_lsp_request_timeout_ms(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum SchemaSource {
    Single(String),
    Multiple(Vec<String>),
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
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum GlobPattern {
    Single(String),
    Multiple(Vec<String>),
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
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProjectConfig {
    pub schema: SchemaSource,
    pub include: GlobPattern,
    pub exclude: Option<GlobPattern>,
    pub output_dir: Option<String>,
    pub import: Option<String>,
    pub generate_permissions: Option<bool>,
    pub codegen: Option<bool>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SchemaTypeConfig {
    pub schema: SchemaSource,
    pub output: String,
    pub import: Option<String>,
}

impl Default for Config {
    /// Returns a default Config with all optional fields set to None.
    ///
    /// This is useful for tests where you only need to set specific fields.
    /// Use the struct update syntax to override specific fields:
    ///
    /// ```rust,ignore
    /// let config = Config {
    ///     base_dir: PathBuf::from("/my/project"),
    ///     projects: vec![...],
    ///     lsp_automatic_codegen: Some(false),
    ///     ..Default::default()
    /// };
    /// ```
    fn default() -> Self {
        Self::new_empty()
    }
}

impl Config {
    /// Creates a new empty config with all fields set to None/default
    pub fn new_empty() -> Self {
        Self {
            output_dir: None,
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
            base_dir: PathBuf::from("."),
        }
    }

    /// Creates a test config with a base directory and projects
    /// All optional fields are set to None, making tests resilient to config changes
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
            if let Some(config) = Self::load_from_dir(&curr) {
                return config;
            }
            if let Some(parent) = curr.parent() {
                curr = parent.to_path_buf();
            } else {
                break;
            }
        }
        eprintln!(
            "{}: No graphql.yaml or graphql.yml found in current or parent directories. This tool requires a configuration file to run.",
            "Error".red()
        );
        std::process::exit(1);
    }

    pub fn load_from_dir<P: AsRef<Path>>(dir: P) -> Option<Self> {
        let dir = dir.as_ref();
        let yaml_path = dir.join("graphql.yaml");
        let yml_path = dir.join("graphql.yml");

        let config_path = if yaml_path.exists() {
            Some(yaml_path)
        } else if yml_path.exists() {
            Some(yml_path)
        } else {
            None
        }?;

        let content = fs::read_to_string(&config_path).ok()?;

        match serde_yaml::from_str::<Config>(&content) {
            Ok(mut config) => {
                config.base_dir = dir.to_path_buf();
                Some(config)
            }
            Err(_) => None,
        }
    }

    pub fn get_project_for_path(&self, path: &Path) -> Option<&ProjectConfig> {
        let abs_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let relative_path = abs_path.strip_prefix(&self.base_dir).ok();

        for project in &self.projects {
            let mut matched = false;
            if let Some(rel_path) = relative_path {
                let mut builder = GlobSetBuilder::new();
                for pattern in project.include.patterns() {
                    if let Ok(glob) = Glob::new(&pattern) {
                        builder.add(glob);
                    }
                }
                if let Ok(set) = builder.build()
                    && set.is_match(rel_path)
                {
                    matched = true;
                }
            }

            if !matched {
                for pattern in project.include.patterns() {
                    let include_path = self.base_dir.join(&pattern);
                    if let Ok(include_path) = fs::canonicalize(include_path)
                        && abs_path.starts_with(&include_path)
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
                let mut builder = GlobSetBuilder::new();
                for pattern in exclude.patterns() {
                    if let Ok(glob) = Glob::new(&pattern) {
                        builder.add(glob);
                    }
                }
                if let Ok(set) = builder.build()
                    && set.is_match(rel_path)
                {
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
}

impl ProjectConfig {
    pub fn codegen_enabled(&self) -> bool {
        self.codegen.unwrap_or(true)
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
        let config_path = dir.path().join("graphql.yaml");
        let mut file = fs::File::create(config_path).unwrap();
        writeln!(
            file,
            r#"
output_dir: "gen"
projects:
  - schema: "s1.graphql"
    include: "src/p1/**/*.ts"
  - schema: "s2.graphql"
    include: "src/p2/**/*.ts"
    output_dir: "gen2"
"#
        )
        .unwrap();

        let config = Config::load_from_dir(dir.path()).unwrap();
        assert_eq!(config.output_dir, Some("gen".to_string()));
        assert_eq!(config.projects.len(), 2);
        assert_eq!(config.projects[0].schema.as_key(), "s1.graphql");
        assert_eq!(config.projects[1].output_dir, Some("gen2".to_string()));
    }

    #[test]
    #[ntest::timeout(100)]
    fn test_load_yml() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("graphql.yml");
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

        let config = Config::load_from_dir(dir.path()).unwrap();
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

        let config_path = parent_dir.join("graphql.yaml");
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
        let config_path = dir.path().join("graphql.yaml");
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

        let config = Config::load_from_dir(dir.path()).unwrap();
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
        let config = Config::load_from_dir(fs::canonicalize(dir.path()).unwrap()).unwrap();

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
        let config_path = dir.path().join("graphql.yaml");
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

        let config = Config::load_from_dir(dir.path()).unwrap();
        assert_eq!(config.projects.len(), 3);

        // First project has codegen disabled
        assert!(!config.projects[0].codegen_enabled());

        // Second project has default (enabled)
        assert!(config.projects[1].codegen_enabled());

        // Third project has codegen explicitly enabled
        assert!(config.projects[2].codegen_enabled());
    }
}
