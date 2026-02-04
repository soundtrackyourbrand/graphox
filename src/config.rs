use fnv::FnvHashMap;
use globset::{Glob, GlobSetBuilder};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub output_dir: Option<String>,
    pub projects: Vec<ProjectConfig>,
    pub schema_types: Option<Vec<SchemaTypeConfig>>,
    pub scalars: Option<FnvHashMap<String, String>>,
    pub ignore_deprecations: Option<Vec<String>>,
    pub generate_ast_for_fragments: Option<bool>,
    pub tracing: Option<TracingConfig>,
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
}

#[derive(Debug, Deserialize, Clone)]
pub struct SchemaTypeConfig {
    pub schema: SchemaSource,
    pub output: String,
    pub import: Option<String>,
}

impl Config {
    pub fn new_empty() -> Self {
        Self {
            output_dir: None,
            projects: vec![],
            schema_types: None,
            scalars: None,
            ignore_deprecations: None,
            generate_ast_for_fragments: None,
            tracing: None,
            base_dir: PathBuf::from("."),
        }
    }

    pub fn load() -> Self {
        let mut curr = std::env::current_dir().unwrap_or_else(|e| {
            eprintln!("Error: Failed to get current directory: {}", e);
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
            "Error: No graphql.yaml or graphql.yml found in current or parent directories. This tool requires a configuration file to run."
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

        let content = fs::read_to_string(&config_path).unwrap_or_else(|e| {
            eprintln!(
                "Error: Failed to read config file {}: {}",
                config_path.display(),
                e
            );
            std::process::exit(1);
        });

        match serde_yaml::from_str::<Config>(&content) {
            Ok(mut config) => {
                config.base_dir = dir.to_path_buf();
                Some(config)
            }
            Err(e) => {
                eprintln!(
                    "Error: Failed to parse config file {}: {}",
                    config_path.display(),
                    e
                );
                std::process::exit(1);
            }
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

            if matched {
                if let Some(exclude) = &project.exclude
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
}
