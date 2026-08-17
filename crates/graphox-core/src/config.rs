use ahash::AHashMap;
use colored::*;
use dashmap::DashMap;
use globset::{Glob, GlobSet, GlobSetBuilder};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use yaml_rust2::Yaml;

static GLOBSET_CACHE: LazyLock<DashMap<Vec<String>, Arc<GlobSet>>> = LazyLock::new(DashMap::new);
const OUTPUT_FILE_CACHE_CAPACITY: usize = 256;

#[derive(Debug, Clone)]
struct PathCandidate {
    raw: PathBuf,
    canonical: Option<PathBuf>,
}

impl PathCandidate {
    fn new(raw: PathBuf) -> Self {
        let canonical = crate::utils::canonicalize_cached(&raw);
        let canonical = (canonical != raw).then_some(canonical);
        Self { raw, canonical }
    }

    fn matches_exact(&self, path: &Path) -> bool {
        crate::utils::paths_match(Some(path), Some(&self.raw))
            || self
                .canonical
                .as_ref()
                .is_some_and(|canonical| crate::utils::paths_match(Some(path), Some(canonical)))
    }

    fn matches_prefix(&self, path: &Path) -> bool {
        crate::utils::path_starts_with(path, &self.raw)
            || self
                .canonical
                .as_ref()
                .is_some_and(|canonical| crate::utils::path_starts_with(path, canonical))
    }
}

#[derive(Debug, Clone, Default)]
struct ResolvedOutputPaths {
    directories: Vec<PathCandidate>,
    files: Vec<PathCandidate>,
}

impl ResolvedOutputPaths {
    fn build(config: &Config) -> Self {
        let mut directories = Vec::new();
        let mut files = Vec::new();

        for project in &config.projects {
            let output_dir = project.output_dir().unwrap_or("__generated__");
            directories.push(PathCandidate::new(config.base_dir.join(output_dir)));

            if let Some(possible_types) = project.possible_types() {
                files.push(PathCandidate::new(config.base_dir.join(possible_types)));
            }
            if let Some(type_policies) = project.type_policies() {
                files.push(PathCandidate::new(config.base_dir.join(type_policies)));
            }
        }

        if let Some(schema_types) = &config.schema_types {
            for schema_type in schema_types {
                files.push(PathCandidate::new(
                    config.base_dir.join(schema_type.output()),
                ));
                if let Some(possible_types) = schema_type.possible_types() {
                    files.push(PathCandidate::new(config.base_dir.join(possible_types)));
                }
                if let Some(type_policies) = schema_type.type_policies() {
                    files.push(PathCandidate::new(config.base_dir.join(type_policies)));
                }
            }
        }

        Self { directories, files }
    }
}

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
    required_fields: Option<AHashMap<String, FieldRule>>,
    forbidden_fields: Option<AHashMap<String, FieldRule>>,
    required_fields_by_type: Option<AHashMap<String, AHashMap<String, FieldRule>>>,
    forbidden_fields_by_type: Option<AHashMap<String, AHashMap<String, FieldRule>>>,
    unique_operation_name: Option<bool>,
    no_duplicate_fields: Option<bool>,
    no_unused_fragments: Option<bool>,
}

pub type RequiredFieldRule = FieldRule;
pub type ForbiddenFieldRule = FieldRule;

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

    pub fn with_required_fields(mut self, fields: AHashMap<String, FieldRule>) -> Self {
        self.required_fields = Some(fields);
        self
    }

    pub fn with_forbidden_fields(mut self, fields: AHashMap<String, FieldRule>) -> Self {
        self.forbidden_fields = Some(fields);
        self
    }

    pub fn required_fields(&self) -> &AHashMap<String, FieldRule> {
        static EMPTY: LazyLock<AHashMap<String, FieldRule>> = LazyLock::new(AHashMap::default);
        self.required_fields.as_ref().unwrap_or(&EMPTY)
    }

    pub fn forbidden_fields(&self) -> &AHashMap<String, FieldRule> {
        static EMPTY: LazyLock<AHashMap<String, FieldRule>> = LazyLock::new(AHashMap::default);
        self.forbidden_fields.as_ref().unwrap_or(&EMPTY)
    }

    pub fn required_fields_by_type(&self) -> &AHashMap<String, AHashMap<String, FieldRule>> {
        static EMPTY: LazyLock<AHashMap<String, AHashMap<String, FieldRule>>> =
            LazyLock::new(AHashMap::default);
        self.required_fields_by_type.as_ref().unwrap_or(&EMPTY)
    }

    pub fn forbidden_fields_by_type(&self) -> &AHashMap<String, AHashMap<String, FieldRule>> {
        static EMPTY: LazyLock<AHashMap<String, AHashMap<String, FieldRule>>> =
            LazyLock::new(AHashMap::default);
        self.forbidden_fields_by_type.as_ref().unwrap_or(&EMPTY)
    }

    pub fn get_required_rule(&self, type_name: &str, field_name: &str) -> Option<&FieldRule> {
        // 1. Check type-specific rule
        if let Some(type_rules) = self.required_fields_by_type().get(type_name)
            && let Some(rule) = type_rules.get(field_name)
        {
            return Some(rule);
        }

        // 2. Fall back to global rule
        self.required_fields().get(field_name)
    }

    pub fn get_forbidden_rule(&self, type_name: &str, field_name: &str) -> Option<&FieldRule> {
        // 1. Check type-specific rule
        if let Some(type_rules) = self.forbidden_fields_by_type().get(type_name)
            && let Some(rule) = type_rules.get(field_name)
        {
            return Some(rule);
        }

        // 2. Fall back to global rule
        self.forbidden_fields().get(field_name)
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

    pub fn merge(&self, other: &Self) -> Self {
        let mut merged = self.clone();

        if other.required_fields.is_some() {
            merged.required_fields = other.required_fields.clone();
        }

        if other.forbidden_fields.is_some() {
            merged.forbidden_fields = other.forbidden_fields.clone();
        }

        if other.required_fields_by_type.is_some() {
            merged.required_fields_by_type = other.required_fields_by_type.clone();
        }

        if other.forbidden_fields_by_type.is_some() {
            merged.forbidden_fields_by_type = other.forbidden_fields_by_type.clone();
        }

        if other.unique_operation_name.is_some() {
            merged.unique_operation_name = other.unique_operation_name;
        }
        if other.no_duplicate_fields.is_some() {
            merged.no_duplicate_fields = other.no_duplicate_fields;
        }
        if other.no_unused_fragments.is_some() {
            merged.no_unused_fragments = other.no_unused_fragments;
        }

        merged
    }

    pub fn from_yaml(node: &Yaml) -> Option<Self> {
        if node.is_null() || node.is_badvalue() {
            return None;
        }

        let mut rules = RulesConfig::default();

        if let Some(rf_hash) = node["required_fields"].as_hash() {
            let mut global_fields = AHashMap::default();
            let mut type_specific_fields = AHashMap::default();

            for (k, v) in rf_hash {
                if let Some(key) = k.as_str() {
                    // Distinguish between field rule and type namespace
                    if v.as_bool().is_some()
                        || v.as_vec().is_some()
                        || v["enabled"].as_str().is_some()
                        || v["enabled"].as_bool().is_some()
                        || v["enabled"].as_vec().is_some()
                    {
                        if let Some(rule) = FieldRule::from_yaml(v) {
                            global_fields.insert(key.to_string(), rule);
                        }
                    } else if let Some(type_hash) = v.as_hash() {
                        // It's a type namespace
                        let mut field_rules = AHashMap::default();
                        for (fk, fv) in type_hash {
                            if let (Some(field_key), Some(rule)) =
                                (fk.as_str(), FieldRule::from_yaml(fv))
                            {
                                field_rules.insert(field_key.to_string(), rule);
                            }
                        }
                        type_specific_fields.insert(key.to_string(), field_rules);
                    }
                }
            }
            rules.required_fields = Some(global_fields);
            rules.required_fields_by_type = Some(type_specific_fields);
        }

        // Handle `required_fields: false` shorthand to disable all required fields
        if let Some(false) = node["required_fields"].as_bool() {
            rules.required_fields = Some(AHashMap::default());
            rules.required_fields_by_type = Some(AHashMap::default());
        }

        if let Some(ff_hash) = node["forbidden_fields"].as_hash() {
            let mut global_fields = AHashMap::default();
            let mut type_specific_fields = AHashMap::default();

            for (k, v) in ff_hash {
                if let Some(key) = k.as_str() {
                    // Distinguish between field rule and type namespace
                    if v.as_bool().is_some()
                        || v.as_vec().is_some()
                        || v["enabled"].as_str().is_some()
                        || v["enabled"].as_bool().is_some()
                        || v["enabled"].as_vec().is_some()
                    {
                        if let Some(rule) = FieldRule::from_yaml(v) {
                            global_fields.insert(key.to_string(), rule);
                        }
                    } else if let Some(type_hash) = v.as_hash() {
                        // It's a type namespace
                        let mut field_rules = AHashMap::default();
                        for (fk, fv) in type_hash {
                            if let (Some(field_key), Some(rule)) =
                                (fk.as_str(), FieldRule::from_yaml(fv))
                            {
                                field_rules.insert(field_key.to_string(), rule);
                            }
                        }
                        type_specific_fields.insert(key.to_string(), field_rules);
                    }
                }
            }
            rules.forbidden_fields = Some(global_fields);
            rules.forbidden_fields_by_type = Some(type_specific_fields);
        }

        // Handle `forbidden_fields: false` shorthand to disable all forbidden fields
        if let Some(false) = node["forbidden_fields"].as_bool() {
            rules.forbidden_fields = Some(AHashMap::default());
            rules.forbidden_fields_by_type = Some(AHashMap::default());
        }

        rules.unique_operation_name = node["unique_operation_name"].as_bool();
        rules.no_duplicate_fields = node["no_duplicate_fields"].as_bool();
        rules.no_unused_fragments = node["no_unused_fragments"].as_bool();

        Some(rules)
    }
}

#[derive(Debug, Clone)]
pub enum FieldEnabled {
    Always(bool),
    Operations(Vec<String>),
}

impl FieldEnabled {
    pub fn applies_to_operation(&self, operation_type: &str) -> bool {
        match self {
            FieldEnabled::Always(enabled) => *enabled,
            FieldEnabled::Operations(ops) => {
                ops.iter().any(|op| op.eq_ignore_ascii_case(operation_type))
            }
        }
    }

    fn from_yaml(node: &Yaml) -> Option<Self> {
        if let Some(b) = node.as_bool() {
            return Some(FieldEnabled::Always(b));
        }
        if let Some(s) = node.as_str() {
            return Some(FieldEnabled::Operations(vec![s.to_string()]));
        }
        if let Some(v) = node.as_vec() {
            let ops = v
                .iter()
                .filter_map(|n| n.as_str().map(String::from))
                .collect();
            return Some(FieldEnabled::Operations(ops));
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct FieldRule {
    enabled: FieldEnabled,
    reason: Option<String>,
}

impl FieldRule {
    pub fn new_always(enabled: bool) -> Self {
        Self {
            enabled: FieldEnabled::Always(enabled),
            reason: None,
        }
    }

    pub fn new_operations(ops: Vec<String>) -> Self {
        Self {
            enabled: FieldEnabled::Operations(ops),
            reason: None,
        }
    }

    pub fn with_reason(mut self, reason: String) -> Self {
        self.reason = Some(reason);
        self
    }

    pub fn applies_to_operation(&self, operation_type: &str) -> bool {
        self.enabled.applies_to_operation(operation_type)
    }

    pub fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }

    fn from_yaml(node: &Yaml) -> Option<Self> {
        if let Some(enabled) = FieldEnabled::from_yaml(node) {
            return Some(FieldRule {
                enabled,
                reason: None,
            });
        }

        if let Some(_map) = node.as_hash() {
            let enabled_node = &node["enabled"];
            let reason = node["reason"].as_str().map(String::from);

            if let Some(enabled) = FieldEnabled::from_yaml(enabled_node) {
                return Some(FieldRule { enabled, reason });
            }
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
pub struct ReactApolloHooksConfig {
    pub enabled: Option<bool>,
    pub common_import_from: Option<String>,
    pub hooks_import_from: Option<String>,
}

#[derive(Debug, Clone, Default, Hash)]
pub struct CodegenConfig {
    pub enabled: Option<bool>,
    pub entrypoint_name: Option<String>,
    pub document_suffix: Option<String>,
    pub omit_operation_suffix_in_document_name: Option<bool>,
    pub variables_suffix: Option<String>,
    pub fragment_suffix: Option<String>,
    pub fragment_document_suffix: Option<String>,
    pub query_suffix: Option<String>,
    pub mutation_suffix: Option<String>,
    pub subscription_suffix: Option<String>,
    pub react_apollo_hooks: Option<Box<ReactApolloHooksConfig>>,
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
    pub nullable_fields_as_optional: Option<bool>,
    pub graphql_tag_fallback: Option<bool>,
    pub merge_union_types: Option<bool>,
    pub prune_orphans: Option<bool>,
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

    pub fn with_omit_operation_suffix_in_document_name(mut self, enabled: bool) -> Self {
        self.omit_operation_suffix_in_document_name = Some(enabled);
        self
    }

    pub fn with_entrypoint_name(mut self, name: String) -> Self {
        self.entrypoint_name = Some(name);
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

    pub fn with_react_apollo_hooks(mut self, enabled: bool) -> Self {
        self.react_apollo_hooks
            .get_or_insert_with(|| Box::new(ReactApolloHooksConfig::default()))
            .enabled = Some(enabled);
        self
    }

    pub fn with_apollo_react_common_import_from(mut self, import_from: String) -> Self {
        self.react_apollo_hooks
            .get_or_insert_with(|| Box::new(ReactApolloHooksConfig::default()))
            .common_import_from = Some(import_from);
        self
    }

    pub fn with_apollo_react_hooks_import_from(mut self, import_from: String) -> Self {
        self.react_apollo_hooks
            .get_or_insert_with(|| Box::new(ReactApolloHooksConfig::default()))
            .hooks_import_from = Some(import_from);
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

    pub fn with_prune_orphans(mut self, enabled: bool) -> Self {
        self.prune_orphans = Some(enabled);
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

    pub fn with_nullable_fields_as_optional(mut self, enabled: bool) -> Self {
        self.nullable_fields_as_optional = Some(enabled);
        self
    }

    pub fn with_graphql_tag_fallback(mut self, enabled: bool) -> Self {
        self.graphql_tag_fallback = Some(enabled);
        self
    }

    pub fn with_merge_union_types(mut self, enabled: bool) -> Self {
        self.merge_union_types = Some(enabled);
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

        let react_apollo_hooks = {
            let enabled = node["react_apollo_hooks"].as_bool();
            let common_import_from = node["apollo_react_common_import_from"]
                .as_str()
                .map(String::from)
                .or_else(|| {
                    node["apolloReactCommonImportFrom"]
                        .as_str()
                        .map(String::from)
                });
            let hooks_import_from = node["apollo_react_hooks_import_from"]
                .as_str()
                .map(String::from)
                .or_else(|| {
                    node["apolloReactHooksImportFrom"]
                        .as_str()
                        .map(String::from)
                });

            if enabled.is_some() || common_import_from.is_some() || hooks_import_from.is_some() {
                Some(Box::new(ReactApolloHooksConfig {
                    enabled,
                    common_import_from,
                    hooks_import_from,
                }))
            } else {
                None
            }
        };

        Some(Self {
            enabled: node["enabled"].as_bool(),
            entrypoint_name: node["entrypoint_name"].as_str().map(String::from),
            document_suffix: node["document_suffix"].as_str().map(String::from),
            omit_operation_suffix_in_document_name: node["omit_operation_suffix_in_document_name"]
                .as_bool(),
            variables_suffix: node["variables_suffix"].as_str().map(String::from),
            fragment_suffix: node["fragment_suffix"].as_str().map(String::from),
            fragment_document_suffix: node["fragment_document_suffix"].as_str().map(String::from),
            query_suffix: node["query_suffix"].as_str().map(String::from),
            mutation_suffix: node["mutation_suffix"].as_str().map(String::from),
            subscription_suffix: node["subscription_suffix"].as_str().map(String::from),
            react_apollo_hooks,
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
            nullable_fields_as_optional: node["nullable_fields_as_optional"].as_bool(),
            graphql_tag_fallback: node["graphql_tag_fallback"].as_bool(),
            merge_union_types: node["merge_union_types"].as_bool(),
            prune_orphans: node["prune_orphans"].as_bool(),
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

    pub fn omit_operation_suffix_in_document_name(&self) -> bool {
        self.omit_operation_suffix_in_document_name.unwrap_or(false)
    }

    pub fn entrypoint_name(&self) -> &str {
        self.entrypoint_name.as_deref().unwrap_or("graphql")
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

    pub fn react_apollo_hooks(&self) -> bool {
        self.react_apollo_hooks
            .as_ref()
            .and_then(|config| config.enabled)
            .unwrap_or(false)
    }

    pub fn apollo_react_common_import_from(&self) -> &str {
        self.react_apollo_hooks
            .as_ref()
            .and_then(|config| config.common_import_from.as_deref())
            .unwrap_or("@apollo/client/react")
    }

    pub fn apollo_react_hooks_import_from(&self) -> &str {
        self.react_apollo_hooks
            .as_ref()
            .and_then(|config| config.hooks_import_from.as_deref())
            .unwrap_or(self.apollo_react_common_import_from())
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

    /// Whether a normal run deletes generated files whose source document is gone.
    /// On by default: leaving them behind breaks `tsc` as soon as a symbol they
    /// import is renamed, and only `--clean` used to remove them.
    pub fn prune_orphans(&self) -> bool {
        self.prune_orphans.unwrap_or(true)
    }

    pub fn emit_permission_data(&self) -> bool {
        self.emit_permission_data.unwrap_or(false)
    }

    pub fn emit_ast_directives(&self) -> bool {
        self.emit_ast_directives.unwrap_or(true)
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

    pub fn nullable_fields_as_optional(&self) -> bool {
        self.nullable_fields_as_optional.unwrap_or(false)
    }

    pub fn graphql_tag_fallback(&self) -> bool {
        self.graphql_tag_fallback.unwrap_or(false)
    }

    pub fn merge_union_types(&self) -> bool {
        self.merge_union_types.unwrap_or(false)
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

    pub fn is_match(&self, path: &Path) -> bool {
        let patterns = self.patterns();
        let set = get_glob_set(&patterns);
        set.is_match(path) || set.is_match(crate::utils::to_posix_path(path))
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
    subgraphs_dir: Option<String>,
    subgraph_owners: Option<AHashMap<String, String>>,
    include: GlobPattern,
    exclude: Option<GlobPattern>,
    output_dir: Option<String>,
    import: Option<String>,
    imports: Option<Vec<String>>,
    codegen_enabled: Option<bool>,
    codegen: Option<CodegenConfig>,
    possible_types: Option<PathBuf>,
    type_policies: Option<PathBuf>,
    rules: Option<RulesConfig>,
}

impl ProjectConfig {
    pub fn with_schema(mut self, schema: SchemaSource) -> Self {
        self.schema = schema;
        self
    }

    pub fn with_subgraphs_dir(mut self, subgraphs_dir: String) -> Self {
        self.subgraphs_dir = Some(subgraphs_dir);
        self
    }

    pub fn with_subgraph_owners(mut self, subgraph_owners: AHashMap<String, String>) -> Self {
        self.subgraph_owners = Some(subgraph_owners);
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

    pub fn subgraphs_dir(&self) -> Option<&str> {
        self.subgraphs_dir.as_deref()
    }

    pub fn subgraph_owners(&self) -> Option<&AHashMap<String, String>> {
        self.subgraph_owners.as_ref()
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

    pub fn imports(&self) -> &[String] {
        self.imports.as_deref().unwrap_or(&[])
    }

    pub fn codegen_enabled(&self) -> Option<bool> {
        self.codegen_enabled
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

    pub fn with_rules(mut self, rules: RulesConfig) -> Self {
        self.rules = Some(rules);
        self
    }

    pub fn rules(&self) -> Option<&RulesConfig> {
        self.rules.as_ref()
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

#[derive(Debug)]
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
    project_cache: Arc<DashMap<PathBuf, Option<usize>>>,
    output_file_cache: Arc<Mutex<crate::utils::BoundedPathCache<bool>>>,
    resolved_output_paths: Arc<OnceLock<ResolvedOutputPaths>>,
    /// Canonicalized absolute paths of every schema file (projects + schema_types),
    /// computed once. Avoids re-`canonicalize`ing schema files for every workspace
    /// document during validation.
    canonical_schema_paths: Arc<OnceLock<Vec<PathBuf>>>,
}

impl Default for Config {
    fn default() -> Self {
        Self::new_empty()
    }
}

impl Clone for Config {
    fn clone(&self) -> Self {
        Self {
            projects: self.projects.clone(),
            schema_types: self.schema_types.clone(),
            scalars: self.scalars.clone(),
            ignore_deprecations: self.ignore_deprecations.clone(),
            tracing: self.tracing.clone(),
            timeouts: self.timeouts.clone(),
            watch_all_files: self.watch_all_files,
            lsp_automatic_codegen: self.lsp_automatic_codegen,
            lsp_codegen_throttle_ms: self.lsp_codegen_throttle_ms,
            codegen_watch_debounce_ms: self.codegen_watch_debounce_ms,
            enable_schema_cache: self.enable_schema_cache,
            rules: self.rules.clone(),
            codegen: self.codegen.clone(),
            base_dir: self.base_dir.clone(),
            // Project matching depends only on immutable config fields, so clones can
            // share the same caches and keep request-scoped config snapshots cheap.
            project_cache: self.project_cache.clone(),
            output_file_cache: self.output_file_cache.clone(),
            resolved_output_paths: self.resolved_output_paths.clone(),
            canonical_schema_paths: self.canonical_schema_paths.clone(),
        }
    }
}

impl Config {
    fn reset_project_cache(&mut self) {
        self.project_cache = Arc::new(DashMap::new());
        self.output_file_cache = Arc::new(Mutex::new(crate::utils::BoundedPathCache::new(
            OUTPUT_FILE_CACHE_CAPACITY,
        )));
        self.resolved_output_paths = Arc::new(OnceLock::new());
        self.canonical_schema_paths = Arc::new(OnceLock::new());
    }

    pub fn with_base_dir(mut self, base_dir: PathBuf) -> Self {
        self.base_dir = std::fs::canonicalize(&base_dir).unwrap_or(base_dir);
        self.reset_project_cache();
        self
    }

    pub fn with_projects(mut self, projects: Vec<ProjectConfig>) -> Self {
        self.projects = projects;
        self.reset_project_cache();
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

    /// Returns the path relative to the base directory, handling platform-specific quirks.
    pub fn relativize(&self, path: &Path) -> PathBuf {
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.base_dir.join(path)
        };

        let abs_path = crate::utils::canonicalize_cached(&abs_path);
        self.relativize_absolute(&abs_path)
    }

    fn relativize_absolute(&self, abs_path: &Path) -> PathBuf {
        #[cfg(windows)]
        {
            let normalized_abs = crate::utils::normalize_windows_path(&abs_path.to_string_lossy());
            let normalized_base =
                crate::utils::normalize_windows_path(&self.base_dir.to_string_lossy());

            pathdiff::diff_paths(&normalized_abs, &normalized_base)
                .unwrap_or_else(|| abs_path.to_path_buf())
        }

        #[cfg(not(windows))]
        {
            pathdiff::diff_paths(abs_path, &self.base_dir).unwrap_or_else(|| abs_path.to_path_buf())
        }
    }

    fn output_cache_key_for_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.base_dir.join(path)
        }
    }

    fn resolved_output_paths(&self) -> &ResolvedOutputPaths {
        self.resolved_output_paths
            .get_or_init(|| ResolvedOutputPaths::build(self))
    }

    /// Canonicalized absolute paths of every configured schema file (across all
    /// projects and `schema_types`), computed once and cached for the lifetime of
    /// this `Config` (and its clones). Used to detect whether a document path is in
    /// fact a schema file without re-`canonicalize`ing the schema set per document.
    pub fn canonical_schema_paths(&self) -> &[PathBuf] {
        self.canonical_schema_paths.get_or_init(|| {
            let mut paths: Vec<PathBuf> = Vec::new();
            let add = |paths: &mut Vec<PathBuf>, file: &str| {
                let abs = self.base_dir.join(file);
                let abs = std::fs::canonicalize(&abs).unwrap_or(abs);
                if !paths.contains(&abs) {
                    paths.push(abs);
                }
            };

            for project in &self.projects {
                for schema_file in project.schema().files() {
                    add(&mut paths, &schema_file);
                }
            }
            if let Some(schema_types) = &self.schema_types {
                for st in schema_types {
                    for schema_file in st.schema().files() {
                        add(&mut paths, &schema_file);
                    }
                }
            }
            paths
        })
    }

    pub fn get_codegen_config(&self, project: Option<&ProjectConfig>) -> CodegenConfig {
        let mut result = self.codegen();

        if let Some(project) = project {
            let project_codegen = project.codegen();
            if project_codegen.entrypoint_name.is_some() {
                result.entrypoint_name = project_codegen.entrypoint_name.clone();
            }
            if project_codegen.document_suffix.is_some() {
                result.document_suffix = project_codegen.document_suffix.clone();
            }
            if project_codegen
                .omit_operation_suffix_in_document_name
                .is_some()
            {
                result.omit_operation_suffix_in_document_name =
                    project_codegen.omit_operation_suffix_in_document_name;
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
            if let Some(project_hooks) = project_codegen.react_apollo_hooks.as_ref() {
                let hooks = result
                    .react_apollo_hooks
                    .get_or_insert_with(|| Box::new(ReactApolloHooksConfig::default()));

                if project_hooks.enabled.is_some() {
                    hooks.enabled = project_hooks.enabled;
                }
                if project_hooks.common_import_from.is_some() {
                    hooks.common_import_from = project_hooks.common_import_from.clone();
                }
                if project_hooks.hooks_import_from.is_some() {
                    hooks.hooks_import_from = project_hooks.hooks_import_from.clone();
                }
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
            if project_codegen.prune_orphans.is_some() {
                result.prune_orphans = project_codegen.prune_orphans;
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
            if project_codegen.nullable_fields_as_optional.is_some() {
                result.nullable_fields_as_optional = project_codegen.nullable_fields_as_optional;
            }
            if project_codegen.graphql_tag_fallback.is_some() {
                result.graphql_tag_fallback = project_codegen.graphql_tag_fallback;
            }
            if project_codegen.merge_union_types.is_some() {
                result.merge_union_types = project_codegen.merge_union_types;
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

    pub fn get_project_index_for_path(&self, path: &Path) -> Option<usize> {
        self.get_project_for_path(path); // Ensure cache is populated
        self.project_cache.get(path).and_then(|v| *v.value())
    }

    pub fn get_project_for_path(&self, path: &Path) -> Option<&ProjectConfig> {
        if let Some(cached) = self.project_cache.get(path) {
            return cached.value().and_then(|idx| self.projects.get(idx));
        }

        let cache_key = self.output_cache_key_for_path(path);
        if cache_key != path
            && let Some(cached) = self.project_cache.get(&cache_key)
        {
            return cached.value().and_then(|idx| self.projects.get(idx));
        }

        let abs_path = crate::utils::canonicalize_cached(&cache_key);

        let relative_path = Some(self.relativize_absolute(&abs_path));

        // First pass: Check if this is exactly a schema file for any project (Highest Priority)
        for (idx, project) in self.projects.iter().enumerate() {
            for schema_file in project.schema().files() {
                let abs_schema = self.base_dir.join(schema_file);
                // Canonicalize schema file path too
                let abs_schema = crate::utils::canonicalize_cached(&abs_schema);
                if crate::utils::paths_match(Some(&abs_path), Some(&abs_schema)) {
                    self.project_cache.insert(cache_key.clone(), Some(idx));
                    return Some(project);
                }
            }
        }

        let mut best_idx = None;
        let mut max_specificity = -1;

        for (idx, project) in self.projects.iter().enumerate() {
            let mut matched = false;
            let mut current_specificity = 0;

            if let Some(rel_path) = &relative_path {
                let posix_rel_path = crate::utils::to_posix_path(rel_path);
                let include_set = get_glob_set(&project.include().patterns());
                if include_set.is_match(&posix_rel_path) || include_set.is_match(rel_path) {
                    matched = true;
                    // Calculate specificity: max depth of glob roots in this project
                    for pattern in project.include().patterns() {
                        let root = crate::utils::get_glob_root(&pattern);
                        let root_len = root.components().count() as i32;
                        current_specificity = current_specificity.max(root_len);
                    }
                } else {
                    for pattern in project.include().patterns() {
                        if !pattern.contains('*')
                            && !pattern.contains('?')
                            && !pattern.contains('[')
                            && !pattern.contains('{')
                        {
                            let pattern_path = Path::new(&pattern);
                            if crate::utils::path_starts_with(rel_path, pattern_path)
                                || posix_rel_path.starts_with(&pattern)
                            {
                                matched = true;
                                current_specificity = current_specificity
                                    .max(pattern_path.components().count() as i32);
                                break;
                            }
                        }
                    }
                }
            }

            if !matched {
                for pattern in project.include().patterns() {
                    let include_path = self.base_dir.join(&pattern);
                    // Only canonicalize if it's a direct path (no globs)
                    if !pattern.contains('*')
                        && !pattern.contains('?')
                        && !pattern.contains('[')
                        && !pattern.contains('{')
                    {
                        let include_path = crate::utils::canonicalize_cached(&include_path);
                        if crate::utils::path_starts_with(&abs_path, &include_path) {
                            matched = true;
                            current_specificity =
                                current_specificity.max(include_path.components().count() as i32);
                            break;
                        }
                    }
                }
            }

            if matched
                && let Some(exclude) = project.exclude()
                && let Some(rel_path) = &relative_path
            {
                let posix_rel_path = crate::utils::to_posix_path(rel_path);
                let exclude_set = get_glob_set(&exclude.patterns());
                if exclude_set.is_match(&posix_rel_path) || exclude_set.is_match(rel_path) {
                    matched = false;
                }
            }

            if matched && current_specificity > max_specificity {
                max_specificity = current_specificity;
                best_idx = Some(idx);
            }
        }

        if let Some(idx) = best_idx {
            self.project_cache.insert(cache_key.clone(), Some(idx));
            return Some(&self.projects[idx]);
        }

        self.project_cache.insert(cache_key, None);
        None
    }

    pub fn is_output_file(&self, path: &Path) -> bool {
        // Check for common generated file extensions
        if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
            if ext == "json" && path.file_name().is_some_and(|n| n == "manifest.json") {
                return true;
            }
            if path.to_string_lossy().ends_with(".codegen.ts") {
                return true;
            }
        }

        let cache_key = self.output_cache_key_for_path(path);
        if let Ok(mut cache) = self.output_file_cache.lock()
            && let Some(cached) = cache.get(&cache_key)
        {
            return cached;
        }

        let abs_path = crate::utils::canonicalize_cached(&cache_key);
        let resolved_paths = self.resolved_output_paths();
        let is_output = resolved_paths
            .directories
            .iter()
            .any(|candidate| candidate.matches_prefix(&abs_path))
            || resolved_paths
                .files
                .iter()
                .any(|candidate| candidate.matches_exact(&abs_path));

        if let Ok(mut cache) = self.output_file_cache.lock() {
            cache.insert(cache_key, is_output);
        }

        is_output
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

    pub fn omit_operation_suffix_in_document_name(&self) -> bool {
        self.codegen
            .as_ref()
            .and_then(|c| c.omit_operation_suffix_in_document_name)
            .unwrap_or(false)
    }

    pub fn entrypoint_name(&self) -> &str {
        static DEFAULT: &str = "graphql";
        self.codegen
            .as_ref()
            .and_then(|c| c.entrypoint_name.as_deref())
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
            project_cache: Arc::new(DashMap::new()),
            output_file_cache: Arc::new(Mutex::new(crate::utils::BoundedPathCache::new(
                OUTPUT_FILE_CACHE_CAPACITY,
            ))),
            resolved_output_paths: Arc::new(OnceLock::new()),
            canonical_schema_paths: Arc::new(OnceLock::new()),
        }
    }

    pub fn new_test(base_dir: PathBuf, projects: Vec<ProjectConfig>) -> Self {
        Self {
            base_dir: fs::canonicalize(&base_dir).unwrap_or(base_dir),
            projects,
            enable_schema_cache: Some(false),
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
            crate::utils::flush_stdio();
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
                        crate::utils::flush_stdio();
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
                    crate::utils::flush_stdio();
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
                // Map YAML keys to ProjectConfig fields. Note that "subgraphs" maps to subgraphs_dir
                // and subgraph_owners is populated from the "subgraph_owners" hash.
                let schema = SchemaSource::from_yaml(&p_node["schema"])?;
                let subgraphs_dir = p_node["subgraphs"].as_str().map(String::from);
                let subgraph_owners = if let Some(owners_hash) = p_node["subgraph_owners"].as_hash()
                {
                    let mut owners = AHashMap::default();
                    for (k, v) in owners_hash {
                        if let (Some(k), Some(v)) = (k.as_str(), v.as_str()) {
                            owners.insert(k.to_string(), v.to_string());
                        }
                    }
                    Some(owners)
                } else {
                    None
                };
                let include = GlobPattern::from_yaml(&p_node["include"])
                    .or_else(|| GlobPattern::from_yaml(&p_node["documents"]))?;
                let exclude = GlobPattern::from_yaml(&p_node["exclude"]);
                let output_dir = p_node["output_dir"].as_str().map(String::from);
                let import = p_node["import"].as_str().map(String::from);
                let imports = p_node["imports"].as_vec().map(|v| {
                    v.iter()
                        .filter_map(|n| n.as_str().map(String::from))
                        .collect()
                });
                let codegen_enabled = p_node["codegen"].as_bool();
                let codegen_config = CodegenConfig::from_yaml(&p_node["codegen"]);
                let possible_types = p_node["possible_types"].as_str().map(PathBuf::from);
                let type_policies = p_node["type_policies"].as_str().map(PathBuf::from);
                let rules = RulesConfig::from_yaml(&p_node["rules"]);

                config.projects.push(ProjectConfig {
                    schema,
                    subgraphs_dir,
                    subgraph_owners,
                    include,
                    exclude,
                    output_dir,
                    import,
                    imports,
                    codegen_enabled,
                    codegen: codegen_config,
                    possible_types,
                    type_policies,
                    rules,
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

        config.rules = RulesConfig::from_yaml(&node["rules"]);

        Some(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn write_test_project(base_dir: &Path) {
        fs::write(base_dir.join("schema.graphql"), "type Query { id: ID! }").expect("write schema");
        fs::create_dir_all(base_dir.join("src")).expect("create src dir");
        fs::write(base_dir.join("src/query.graphql"), "query Test { id }").expect("write query");
    }

    fn make_test_config(base_dir: &Path) -> Config {
        write_test_project(base_dir);
        Config::new_test(
            base_dir.to_path_buf(),
            vec![
                ProjectConfig::default()
                    .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                    .with_include(GlobPattern::Single("src/query.graphql".to_string())),
            ],
        )
    }

    fn make_output_test_config(base_dir: &Path) -> Config {
        write_test_project(base_dir);
        Config::new_test(
            base_dir.to_path_buf(),
            vec![
                ProjectConfig::default()
                    .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                    .with_include(GlobPattern::Single("src/query.graphql".to_string()))
                    .with_output_dir("gen".to_string()),
            ],
        )
    }

    #[test]
    fn config_clone_shares_project_cache() {
        let temp_dir = tempdir().expect("create temp dir");
        let config = make_test_config(temp_dir.path());
        let query_path = temp_dir.path().join("src/query.graphql");

        assert!(config.get_project_for_path(&query_path).is_some());
        assert_eq!(config.project_cache.len(), 1);

        let cloned = config.clone();

        assert!(Arc::ptr_eq(&config.project_cache, &cloned.project_cache));
        assert_eq!(cloned.project_cache.len(), 1);
        assert!(cloned.get_project_for_path(&query_path).is_some());
        assert_eq!(cloned.project_cache.len(), 1);
    }

    #[test]
    fn config_clone_shares_output_file_cache() {
        let temp_dir = tempdir().expect("create temp dir");
        let config = make_output_test_config(temp_dir.path());
        let source_path = temp_dir.path().join("src/query.graphql");

        assert!(!config.is_output_file(&source_path));
        assert_eq!(config.output_file_cache.lock().unwrap().len(), 1);

        let cloned = config.clone();

        assert!(Arc::ptr_eq(
            &config.output_file_cache,
            &cloned.output_file_cache
        ));
        assert_eq!(cloned.output_file_cache.lock().unwrap().len(), 1);
        assert!(!cloned.is_output_file(&source_path));
        assert_eq!(cloned.output_file_cache.lock().unwrap().len(), 1);
    }

    #[test]
    fn with_base_dir_resets_project_cache() {
        let temp_dir = tempdir().expect("create temp dir");
        let other_dir = tempdir().expect("create other temp dir");
        let config = make_test_config(temp_dir.path());
        let query_path = temp_dir.path().join("src/query.graphql");

        assert!(config.get_project_for_path(&query_path).is_some());
        assert_eq!(config.project_cache.len(), 1);

        let updated = config.clone().with_base_dir(other_dir.path().to_path_buf());

        assert!(!Arc::ptr_eq(&config.project_cache, &updated.project_cache));
        assert!(updated.project_cache.is_empty());
    }

    #[test]
    fn with_base_dir_resets_output_file_cache() {
        let temp_dir = tempdir().expect("create temp dir");
        let other_dir = tempdir().expect("create other temp dir");
        let config = make_output_test_config(temp_dir.path());
        let source_path = temp_dir.path().join("src/query.graphql");

        assert!(!config.is_output_file(&source_path));
        assert_eq!(config.output_file_cache.lock().unwrap().len(), 1);

        let updated = config.clone().with_base_dir(other_dir.path().to_path_buf());

        assert!(!Arc::ptr_eq(
            &config.output_file_cache,
            &updated.output_file_cache
        ));
        assert_eq!(updated.output_file_cache.lock().unwrap().len(), 0);
    }

    #[test]
    fn with_projects_resets_project_cache() {
        let temp_dir = tempdir().expect("create temp dir");
        let config = make_test_config(temp_dir.path());
        let query_path = temp_dir.path().join("src/query.graphql");

        assert!(config.get_project_for_path(&query_path).is_some());
        assert_eq!(config.project_cache.len(), 1);

        let updated = config.clone().with_projects(vec![]);

        assert!(!Arc::ptr_eq(&config.project_cache, &updated.project_cache));
        assert!(updated.project_cache.is_empty());
    }

    #[test]
    fn with_projects_resets_output_file_cache() {
        let temp_dir = tempdir().expect("create temp dir");
        let config = make_output_test_config(temp_dir.path());
        let source_path = temp_dir.path().join("src/query.graphql");

        assert!(!config.is_output_file(&source_path));
        assert_eq!(config.output_file_cache.lock().unwrap().len(), 1);

        let updated = config.clone().with_projects(vec![]);

        assert!(!Arc::ptr_eq(
            &config.output_file_cache,
            &updated.output_file_cache
        ));
        assert_eq!(updated.output_file_cache.lock().unwrap().len(), 0);
    }

    #[test]
    fn canonical_schema_paths_resolves_and_dedupes() {
        let temp_dir = tempdir().expect("create temp dir");
        let base = temp_dir.path().canonicalize().expect("canonicalize base");
        write_test_project(&base);

        // Two projects share the same schema file — it must appear only once.
        let config = Config::new_test(
            base.clone(),
            vec![
                ProjectConfig::default()
                    .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                    .with_include(GlobPattern::Single("a/**/*.graphql".to_string())),
                ProjectConfig::default()
                    .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                    .with_include(GlobPattern::Single("b/**/*.graphql".to_string())),
            ],
        );

        let schema_paths = config.canonical_schema_paths();
        assert_eq!(schema_paths.len(), 1, "shared schema must be deduped");
        assert_eq!(
            schema_paths[0],
            base.join("schema.graphql").canonicalize().unwrap()
        );
    }

    #[test]
    fn with_projects_resets_canonical_schema_paths() {
        let temp_dir = tempdir().expect("create temp dir");
        let base = temp_dir.path().canonicalize().expect("canonicalize base");
        let config = make_test_config(&base);

        // Populate the lazy cache.
        assert_eq!(config.canonical_schema_paths().len(), 1);

        let updated = config.clone().with_projects(vec![]);
        assert!(!Arc::ptr_eq(
            &config.canonical_schema_paths,
            &updated.canonical_schema_paths
        ));
        assert!(updated.canonical_schema_paths().is_empty());
    }
}
