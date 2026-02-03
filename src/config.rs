use glob::Pattern;
use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub output_dir: Option<String>,
    pub projects: Vec<ProjectConfig>,
    pub schema_types: Option<Vec<SchemaTypeConfig>>,
    pub scalars: Option<fnv::FnvHashMap<String, String>>,
    pub ignore_deprecations: Option<Vec<String>>,
    #[serde(skip)]
    pub base_dir: std::path::PathBuf,
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
pub struct ProjectConfig {
    pub schema: SchemaSource,
    pub include: String,
    pub output_dir: Option<String>,
    pub import: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SchemaTypeConfig {
    pub schema: SchemaSource,
    pub output: String,
    pub import: Option<String>,
}

impl Config {
    pub fn load() -> Option<Self> {
        let mut curr = std::env::current_dir().ok()?;
        loop {
            if let Some(config) = Self::load_from_dir(&curr) {
                return Some(config);
            }
            if let Some(parent) = curr.parent() {
                curr = parent.to_path_buf();
            } else {
                break;
            }
        }
        None
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
            eprintln!("Error: Failed to read config file {}: {}", config_path.display(), e);
            std::process::exit(1);
        });

        match serde_yaml::from_str::<Config>(&content) {
            Ok(mut config) => {
                config.base_dir = dir.to_path_buf();
                Some(config)
            }
            Err(e) => {
                eprintln!("Error: Failed to parse config file {}: {}", config_path.display(), e);
                std::process::exit(1);
            }
        }
    }

    pub fn get_schema_for_path(&self, path: &Path) -> Option<String> {
        let abs_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let relative_path = abs_path.strip_prefix(&self.base_dir).ok();

        for project in &self.projects {
            if let Some(rel_path) = relative_path {
                if let Ok(pattern) = Pattern::new(&project.include)
                    && pattern.matches_path(rel_path)
                {
                    return Some(project.schema.as_key());
                }
            }
            // Fallback for non-glob paths
            let include_path = self.base_dir.join(&project.include);
            if let Ok(include_path) = fs::canonicalize(include_path)
                && abs_path.starts_with(&include_path)
            {
                return Some(project.schema.as_key());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
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
    fn test_no_config() {
        let dir = tempdir().unwrap();
        let config = Config::load_from_dir(dir.path());
        assert!(config.is_none());
    }

    #[test]
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

        let config = config.expect("Should find config in parent directory");
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
}
