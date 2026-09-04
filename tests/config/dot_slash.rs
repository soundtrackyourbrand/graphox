//! A leading `./` on a config glob pattern must not silently match zero files.

use graphox::Config;
use graphox::config::GlobPattern;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

#[test]
#[ntest::timeout(3000)]
fn test_glob_pattern_strips_leading_dot_slash() {
    let plain = GlobPattern::Single("src/**/*.graphql".to_string());
    let dotted = GlobPattern::Single("./src/**/*.graphql".to_string());

    assert_eq!(dotted.patterns(), plain.patterns());

    let path = Path::new("src/features/ops.graphql");
    assert!(plain.is_match(path));
    assert!(dotted.is_match(path), "`./`-prefixed pattern should match");
}

#[test]
#[ntest::timeout(3000)]
fn test_glob_pattern_strips_leading_dot_slash_in_multiple() {
    let dotted = GlobPattern::Multiple(vec![
        "./src/**/*.graphql".to_string(),
        "lib/**/*.graphql".to_string(),
    ]);

    assert_eq!(
        dotted.patterns(),
        vec!["src/**/*.graphql", "lib/**/*.graphql"]
    );
    assert!(dotted.is_match(Path::new("src/features/ops.graphql")));
    assert!(dotted.is_match(Path::new("lib/ops.graphql")));
}

#[test]
#[ntest::timeout(3000)]
fn test_glob_pattern_leaves_other_patterns_alone() {
    // Only a leading `./` is stripped; `..`, bare names and absolute paths are untouched.
    for pattern in [
        "**/*.graphql",
        "../shared/**/*.graphql",
        "/abs/**/*.graphql",
    ] {
        let p = GlobPattern::Single(pattern.to_string());
        assert_eq!(p.patterns(), vec![pattern.to_string()]);
    }
}

/// Write a project whose `documents` pattern carries a `./` prefix and confirm
/// the file is both collected by the walker and attributed back to the project.
/// Those are two separate code paths, and the pattern is used differently in
/// each: matched against a relative path, and joined onto the base dir.
#[test]
#[ntest::timeout(3000)]
fn test_dot_slash_documents_pattern_collects_and_attributes_files() {
    let dir = TempDir::new().unwrap();
    let base = dir.path();

    fs::write(base.join("schema.graphql"), "type Query { a: String }").unwrap();
    fs::create_dir_all(base.join("src/features")).unwrap();
    let doc_path = base.join("src/features/ops.graphql");
    fs::write(&doc_path, "query Q { a }").unwrap();

    fs::write(
        base.join("graphox.yaml"),
        r#"
projects:
  - schema: ./schema.graphql
    documents: ./src/**/*.graphql
    codegen:
      enabled: false
"#,
    )
    .unwrap();

    let config = Config::load_from_dir(base)
        .expect("config should load")
        .expect("config should be present");

    let project = &config.projects()[0];

    let files = graphox::utils::get_project_scan_files(&config, project, None);
    assert_eq!(
        files.len(),
        1,
        "expected the walker to collect the document, got {:?}",
        files
    );

    assert!(
        config.get_project_for_path(&doc_path).is_some(),
        "collected document must resolve back to its project"
    );
}
