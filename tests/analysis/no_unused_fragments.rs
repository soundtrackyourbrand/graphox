use crate::support::assert_diag_range_equals;
use crate::support::assert_diagnostic_with_message;
use crate::support::assert_diagnostics_count;
use crate::support::create_doc;
use ahash::AHashSet;
use graphox::Config;
use graphox::config::{GlobPattern, ProjectConfig, RulesConfig, SchemaSource};
use graphox::features::diagnostics::DocumentDiagnostics;
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;

#[test]
#[ntest::timeout(300)]
fn test_unused_fragment_reported_when_enabled() {
    let dir = tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();

    fs::write(base.join("package.json"), "{}").unwrap();
    fs::write(
        base.join("schema.graphql"),
        "type Query { me: User } type User { id: ID name: String }",
    )
    .unwrap();

    let frag_text = "fragment UnusedFragment on User { id }";
    let frag_path = base.join("unused.graphql");
    fs::write(&frag_path, frag_text).unwrap();

    let config = Config::new_test(
        base.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("**/*.graphql".to_string())),
        ],
    )
    .with_rules(RulesConfig::default().with_no_unused_fragments(true));

    let schema_text = fs::read_to_string(base.join("schema.graphql")).unwrap();
    let schema = apollo_compiler::Schema::parse(&schema_text, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let uri = graphox::utils::path_to_uri(&frag_path).unwrap();
    let doc = create_doc(uri.as_str(), frag_text);
    let used_fragments: AHashSet<Arc<str>> = AHashSet::new();
    let diags = doc.get_semantic_diagnostics(
        &schema,
        &[],
        Some(&used_fragments),
        Some(&config),
        false,
        true,
    );

    assert_diagnostics_count(&diags, 1);
    let diag = assert_diagnostic_with_message(&diags, "Unused fragment: UnusedFragment");
    assert_diag_range_equals(
        diag,
        &crate::support::range_for_token(&doc, frag_text, "UnusedFragment"),
    );
}

#[test]
#[ntest::timeout(300)]
fn test_unused_fragment_not_reported_when_disabled() {
    let dir = tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();

    fs::write(base.join("package.json"), "{}").unwrap();
    fs::write(
        base.join("schema.graphql"),
        "type Query { me: User } type User { id: ID name: String }",
    )
    .unwrap();

    let frag_text = "fragment UnusedFragment on User { id }";
    let frag_path = base.join("unused.graphql");
    fs::write(&frag_path, frag_text).unwrap();

    let config = Config::new_test(
        base.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("**/*.graphql".to_string())),
        ],
    )
    .with_rules(RulesConfig::default().with_no_unused_fragments(false));

    let schema_text = fs::read_to_string(base.join("schema.graphql")).unwrap();
    let schema = apollo_compiler::Schema::parse(&schema_text, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let uri = graphox::utils::path_to_uri(&frag_path).unwrap();
    let doc = create_doc(uri.as_str(), frag_text);
    let used_fragments: AHashSet<Arc<str>> = AHashSet::new();
    let diags = doc.get_semantic_diagnostics(
        &schema,
        &[],
        Some(&used_fragments),
        Some(&config),
        false,
        true,
    );

    assert_diagnostics_count(&diags, 0);
}

#[test]
#[ntest::timeout(300)]
fn test_unused_fragment_not_reported_when_not_configured() {
    let dir = tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();

    fs::write(base.join("package.json"), "{}").unwrap();
    fs::write(
        base.join("schema.graphql"),
        "type Query { me: User } type User { id: ID name: String }",
    )
    .unwrap();

    let frag_text = "fragment UnusedFragment on User { id }";
    let frag_path = base.join("unused.graphql");
    fs::write(&frag_path, frag_text).unwrap();

    let config = Config::new_test(
        base.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("**/*.graphql".to_string())),
        ],
    );

    let schema_text = fs::read_to_string(base.join("schema.graphql")).unwrap();
    let schema = apollo_compiler::Schema::parse(&schema_text, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let uri = graphox::utils::path_to_uri(&frag_path).unwrap();
    let doc = create_doc(uri.as_str(), frag_text);
    let used_fragments: AHashSet<Arc<str>> = AHashSet::new();
    let diags = doc.get_semantic_diagnostics(
        &schema,
        &[],
        Some(&used_fragments),
        Some(&config),
        false,
        true,
    );

    assert_diagnostics_count(&diags, 0);
}

#[test]
#[ntest::timeout(300)]
fn test_used_fragment_not_reported() {
    let dir = tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();

    fs::write(base.join("package.json"), "{}").unwrap();
    fs::write(
        base.join("schema.graphql"),
        "type Query { me: User } type User { id: ID name: String }",
    )
    .unwrap();

    let frag_text = "fragment UsedFragment on User { id }";
    let frag_path = base.join("used.graphql");
    fs::write(&frag_path, frag_text).unwrap();

    let query_text = "query { me { ...UsedFragment } }";
    let query_path = base.join("query.graphql");
    fs::write(&query_path, query_text).unwrap();

    let config = Config::new_test(
        base.clone(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("**/*.graphql".to_string())),
        ],
    )
    .with_rules(RulesConfig::default().with_no_unused_fragments(true));

    let schema_text = fs::read_to_string(base.join("schema.graphql")).unwrap();
    let schema = apollo_compiler::Schema::parse(&schema_text, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let frag_uri = graphox::utils::path_to_uri(&frag_path).unwrap();
    let frag_doc = create_doc(frag_uri.as_str(), frag_text);
    let mut used_fragments: AHashSet<Arc<str>> = AHashSet::new();
    used_fragments.insert("UsedFragment".into());
    let diags = frag_doc.get_semantic_diagnostics(
        &schema,
        &[],
        Some(&used_fragments),
        Some(&config),
        false,
        true,
    );

    assert_diagnostics_count(&diags, 0);
}
