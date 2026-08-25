//! Builders for creating complex test objects with less boilerplate.

use graphox::Config;
use graphox::config::{CodegenConfig, GlobPattern, ProjectConfig, SchemaSource};
use std::path::{Path, PathBuf};

// =============================================================================
// Config Builder
// =============================================================================

/// Builder for creating Config objects in tests.
///
/// # Example
///
/// ```
/// use tests::ConfigBuilder;
///
/// let config = ConfigBuilder::new(&base_dir)
///     .add_project(ProjectConfigBuilder::new().single_schema("schema.graphql"))
///     .build();
/// ```
pub struct ConfigBuilder {
    base_dir: PathBuf,
    projects: Vec<ProjectConfigBuilder>,
    enable_schema_cache: Option<bool>,
    lsp_automatic_codegen: Option<bool>,
}

impl ConfigBuilder {
    /// Create a new builder with the given base directory.
    pub fn new(base_dir: &Path) -> Self {
        Self {
            base_dir: base_dir.to_path_buf(),
            projects: Vec::new(),
            enable_schema_cache: Some(false),
            lsp_automatic_codegen: Some(false),
        }
    }

    /// Add a project configuration.
    pub fn add_project(mut self, project: ProjectConfigBuilder) -> Self {
        self.projects.push(project);
        self
    }

    /// Enable or disable schema caching.
    pub fn enable_schema_cache(mut self, enabled: bool) -> Self {
        self.enable_schema_cache = Some(enabled);
        self
    }

    /// Enable or disable automatic LSP codegen.
    pub fn lsp_automatic_codegen(mut self, enabled: bool) -> Self {
        self.lsp_automatic_codegen = Some(enabled);
        self
    }

    /// Build the Config object.
    pub fn build(self) -> Config {
        let mut config = Config::new_test(
            self.base_dir,
            self.projects.into_iter().map(|p| p.build()).collect(),
        );
        if let Some(enabled) = self.enable_schema_cache {
            config = config.with_enable_schema_cache(enabled);
        }
        if let Some(enabled) = self.lsp_automatic_codegen {
            config = config.with_lsp_automatic_codegen(enabled);
        }
        config
    }
}

// =============================================================================
// ProjectConfig Builder
// =============================================================================

/// Builder for creating ProjectConfig objects in tests.
///
/// # Example
///
/// ```
/// use tests::ProjectConfigBuilder;
///
/// let project = ProjectConfigBuilder::new()
///     .single_schema("schema.graphql")
///     .include_pattern("**/*.graphql")
///     .build();
/// ```
pub struct ProjectConfigBuilder {
    schema: SchemaSource,
    include: GlobPattern,
    exclude: Option<GlobPattern>,
    codegen: Option<CodegenConfig>,
}

impl ProjectConfigBuilder {
    /// Create a new builder with default settings.
    pub fn new() -> Self {
        Self {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            codegen: Some(CodegenConfig::disabled()),
        }
    }

    /// Set a single schema file.
    pub fn single_schema(mut self, schema_path: &str) -> Self {
        self.schema = SchemaSource::Single(schema_path.to_string());
        self
    }

    /// Set multiple schema files (for schema merging).
    pub fn multi_schema(mut self, schemas: Vec<String>) -> Self {
        self.schema = SchemaSource::Multiple(schemas);
        self
    }

    /// Set the include glob pattern.
    pub fn include_pattern(mut self, pattern: &str) -> Self {
        self.include = GlobPattern::Single(pattern.to_string());
        self
    }

    /// Set the exclude glob pattern.
    pub fn exclude_pattern(mut self, pattern: &str) -> Self {
        self.exclude = Some(GlobPattern::Single(pattern.to_string()));
        self
    }

    /// Enable or disable codegen.
    pub fn codegen(mut self, enabled: bool) -> Self {
        self.codegen = if enabled {
            Some(CodegenConfig::enabled())
        } else {
            Some(CodegenConfig::disabled())
        };
        self
    }

    /// Build the ProjectConfig object.
    pub fn build(self) -> ProjectConfig {
        let mut project = ProjectConfig::default()
            .with_schema(self.schema)
            .with_include(self.include);
        if let Some(exclude) = self.exclude {
            project = project.with_exclude(exclude);
        }
        if let Some(codegen) = self.codegen {
            project = project.with_codegen(codegen);
        }
        project
    }
}

impl Default for ProjectConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// FragmentCompletionInfo Builder
// =============================================================================

use graphox::features::completion::FragmentCompletionInfo;
use std::collections::BTreeMap;
use std::sync::Arc;
use tower_lsp_server::ls_types::Uri;

/// Builder for creating FragmentCompletionInfo objects in tests.
///
/// # Example
///
/// ```
/// use tests::FragmentInfoBuilder;
///
/// let frag = FragmentInfoBuilder::new("UserFields", "User")
///     .with_uri(uri)
///     .public()
///     .build();
/// ```
pub struct FragmentInfoBuilder {
    name: String,
    type_condition: String,
    is_public: bool,
    uri: Uri,
    package_root: Option<PathBuf>,
}

impl FragmentInfoBuilder {
    /// Create a new builder with name and type condition.
    pub fn new(name: &str, type_condition: &str) -> Self {
        Self {
            name: name.to_string(),
            type_condition: type_condition.to_string(),
            is_public: false,
            uri: "file:///test.graphql".parse::<Uri>().unwrap(),
            package_root: None,
        }
    }

    /// Mark the fragment as public.
    pub fn public(mut self) -> Self {
        self.is_public = true;
        self
    }

    /// Set the URI for the fragment.
    pub fn with_uri(mut self, uri: Uri) -> Self {
        self.uri = uri;
        self
    }

    /// Set the package root.
    pub fn with_package_root(mut self, path: PathBuf) -> Self {
        self.package_root = Some(path);
        self
    }

    /// Build the FragmentCompletionInfo object.
    pub fn build(self) -> FragmentCompletionInfo {
        FragmentCompletionInfo {
            name: self.name.into(),
            type_condition: self.type_condition.into(),
            description: None,
            import_path: None,
            is_public: self.is_public,
            is_type_only: false,
            uri: self.uri,
            package_root: self.package_root,
            used_variables: Arc::from([]),
            used_fragments: Arc::from([]),
            transitive_deps: Arc::from([]),
            selected_fields: Arc::from([]),
            top_level_spreads: Arc::from([]),
            nested_selections: Arc::from([]),
            selection_ignores: Arc::from([]),
            type_fields: Arc::from([]),
            requirements: BTreeMap::new(),
            worst_slo: None,
        }
    }
}
