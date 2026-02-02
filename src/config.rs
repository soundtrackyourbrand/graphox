use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub output_dir: Option<String>,
    pub projects: Vec<ProjectConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProjectConfig {
    pub schema: String,
    pub include: String,
    pub output_dir: Option<String>,
}

impl Config {
    pub fn load() -> Option<Self> {
        Self::load_from_dir(std::env::current_dir().ok()?)
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

        let content = fs::read_to_string(config_path).ok()?;
        serde_yaml::from_str(&content).ok()
    }

    pub fn get_schema_for_path(&self, path: &Path) -> Option<String> {
        use glob::Pattern;
        let abs_path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());

        for project in &self.projects {
            if let Ok(pattern) = Pattern::new(&project.include) {
                if pattern.matches_path(&abs_path) {
                    return Some(project.schema.clone());
                }
            }
            // Fallback for non-glob paths
            if let Ok(include_path) = fs::canonicalize(&project.include) {
                if abs_path.starts_with(&include_path) {
                    return Some(project.schema.clone());
                }
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
        assert_eq!(config.projects[0].schema, "s1.graphql");
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
        assert_eq!(config.projects[0].schema, "s.graphql");
    }

    #[test]
    fn test_no_config() {
        let dir = tempdir().unwrap();
        let config = Config::load_from_dir(dir.path());
        assert!(config.is_none());
    }
}
