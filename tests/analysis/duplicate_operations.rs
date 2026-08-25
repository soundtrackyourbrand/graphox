use crate::support::{
    create_doc, create_initialized_lsp_service, lsp_did_open, lsp_request_diagnostics,
    range_for_token_at_index, write_project_file,
};
use apollo_compiler::Schema;
use graphox::features::diagnostics::DocumentDiagnostics;
use graphox::{
    Config,
    config::{CodegenConfig, GlobPattern, ProjectConfig, RulesConfig, SchemaSource},
};
use std::fs;
use tempfile::TempDir;
use tempfile::tempdir;
use tower_lsp_server::ls_types::{
    DocumentDiagnosticReport, DocumentDiagnosticReportResult, NumberOrString,
};

fn get_schema() -> apollo_compiler::validation::Valid<Schema> {
    let schema_text = r#"
        type Query {
            user(id: ID!): User
        }

        type User {
            id: ID!
            name: String!
        }
    "#;
    Schema::parse(schema_text, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap()
}

#[test]
#[ntest::timeout(3000)]
fn test_duplicate_operation_names_same_file() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("queries.graphql");

    let content = r#"
        query GetUser($id: ID) {
            user(id: $id) {
                id
                name
            }
        }

        query GetUser($id: ID) {
            user(id: $id) {
                id
            }
        }
    "#;

    std::fs::write(&file_path, content).unwrap();

    let uri = graphox::utils::path_to_uri(&file_path).unwrap();
    let doc = create_doc(uri.as_str(), content);
    let schema = get_schema();

    let config = Config::new_test(
        dir.path().to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("**/*.graphql".to_string())),
        ],
    )
    .with_rules(RulesConfig::default().with_unique_operation_name(true));

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    // Only expect our internal diagnostic now (reported once per name per file)
    assert_eq!(diagnostics.len(), 1);

    let d = &diagnostics[0];
    assert_eq!(d.message, "Duplicate operation name 'GetUser'");
    // First GetUser name
    crate::support::assert_diag_range_equals(
        d,
        &range_for_token_at_index(&doc, content, "GetUser", 0),
    );
}

#[test]
#[ntest::timeout(3000)]
fn test_unique_operation_names_no_error() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("queries.graphql");

    let content = r#"
        query GetUser {
            user(id: "1") {
                id
                name
            }
        }

        query GetAllUsers {
            user(id: "2") {
                id
            }
        }
    "#;

    std::fs::write(&file_path, content).unwrap();

    let uri = graphox::utils::path_to_uri(&file_path).unwrap();
    let doc = create_doc(uri.as_str(), content);
    let schema = get_schema();

    let config = Config::new_test(
        dir.path().to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("**/*.graphql".to_string())),
        ],
    )
    .with_rules(RulesConfig::default().with_unique_operation_name(true));

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    // Should NOT have diagnostics for duplicate operations
    // Check there are no duplicate_operation diagnostics
    let duplicate_errors: Vec<_> = diagnostics
        .iter()
        .filter(
            |d| matches!(d.code, Some(NumberOrString::String(ref s)) if s == "duplicate_operation"),
        )
        .collect();

    assert!(
        duplicate_errors.is_empty(),
        "Should not detect duplicates when names are unique"
    );
}

#[test]
#[ntest::timeout(3000)]
fn test_duplicate_operation_rule_disabled() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("queries.graphql");

    let content = r#"
        query GetUser {
            user(id: "1") {
                id
                name
            }
        }

        query GetUser {
            user(id: "2") {
                id
            }
        }
    "#;

    std::fs::write(&file_path, content).unwrap();

    let uri = graphox::utils::path_to_uri(&file_path).unwrap();
    let doc = create_doc(uri.as_str(), content);
    let schema = get_schema();

    let config = Config::new_test(
        dir.path().to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("**/*.graphql".to_string())),
        ],
    )
    .with_rules(RulesConfig::default().with_unique_operation_name(false));

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    // Should NOT have diagnostics when rule is disabled
    let duplicate_errors: Vec<_> = diagnostics
        .iter()
        .filter(
            |d| matches!(d.code, Some(NumberOrString::String(ref s)) if s == "duplicate_operation"),
        )
        .collect();

    assert!(
        duplicate_errors.is_empty(),
        "Should not detect duplicates when rule is disabled"
    );
}

#[test]
#[ntest::timeout(3000)]
fn test_duplicate_operation_no_rules_config() {
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("queries.graphql");

    let content = r#"
        query GetUser {
            user(id: "1") {
                id
                name
            }
        }

        query GetUser {
            user(id: "2") {
                id
            }
        }
    "#;

    std::fs::write(&file_path, content).unwrap();

    let uri = graphox::utils::path_to_uri(&file_path).unwrap();
    let doc = create_doc(uri.as_str(), content);
    let schema = get_schema();

    let config = Config::new_test(
        dir.path().to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("**/*.graphql".to_string())),
        ],
    );

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    // Should NOT have diagnostics when no rules config exists
    let duplicate_errors: Vec<_> = diagnostics
        .iter()
        .filter(
            |d| matches!(d.code, Some(NumberOrString::String(ref s)) if s == "duplicate_operation"),
        )
        .collect();

    assert!(
        duplicate_errors.is_empty(),
        "Should not detect duplicates when rules config is not provided"
    );
}

#[tokio::test]
#[ntest::timeout(1000)]
async fn test_duplicate_operation_name_on_reopen() {
    let dir = TempDir::new().expect("failed to create tempdir");
    let schema_text = "type Query { foo: String }";
    let schema_path = dir.path().join("schema.graphql");
    fs::write(&schema_path, schema_text).expect("write schema");

    let query_text = "query GetFoo { foo }";
    let query_uri = write_project_file(&dir, "query.graphql", query_text);

    let config = Config::new_test(
        dir.path().to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_rules(RulesConfig::default().with_unique_operation_name(true))
    .with_enable_schema_cache(false)
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Open the file.
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let mut diagnostics = Vec::new();
    let start = std::time::Instant::now();
    while start.elapsed().as_millis() < 300 {
        let result = lsp_request_diagnostics(&mut service, query_uri.clone()).await;
        if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) =
            result
        {
            diagnostics = full_report.full_document_diagnostic_report.items;
            if !diagnostics.is_empty() {
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    let has_dup = diagnostics
        .iter()
        .any(|d| d.message.contains("Duplicate operation name"));
    assert!(
        !has_dup,
        "LSP should NOT report duplicate operation name after opening a file that was already scanned. Diagnostics: {:?}",
        diagnostics
    );
}
