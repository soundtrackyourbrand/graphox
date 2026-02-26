use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, write_project_file,
};
use graphox::CodegenConfig;
use graphox::config::{Config, GlobPattern, ProjectConfig, SchemaSource};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(5000)]
async fn test_goto_definition_scopes_to_correct_schema() {
    let dir = tempfile::TempDir::new().expect("failed to create tempdir");
    let base_dir = dir.path().to_path_buf();

    // Schema A
    let schema_a = "type Query { user: User }

type User {
  id: ID!
}";
    std::fs::create_dir_all(base_dir.join("project-a")).unwrap();
    std::fs::write(base_dir.join("project-a/schema.graphql"), schema_a).expect("write schema-a");

    // Schema B
    let schema_b = "type Query { user: User }

type User {
  username: String!
}";
    std::fs::create_dir_all(base_dir.join("project-b")).unwrap();
    std::fs::write(base_dir.join("project-b/schema.graphql"), schema_b).expect("write schema-b");

    let config = Config::new_test(
        base_dir.clone(),
        vec![
            // Broad project that matches everything
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("project-b/schema.graphql".to_string()))
                .with_include(GlobPattern::Single("**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("project-a/schema.graphql".to_string()))
                .with_include(GlobPattern::Single("project-a/**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Test 1: Go to definition from a query in Project A
    let query_a = "query { user { id } }";
    let query_a_uri = write_project_file(&dir, "project-a/query.graphql", query_a);
    lsp_did_open(&mut service, query_a_uri.clone(), "graphql", 1, query_a).await;

    // "user" is at line 0, col 8. "User" type is what we want.
    // Wait, "user" field definition in schema-a is at line 0.
    // Let's click on "user" in the query.
    let pos = Position::new(0, 8);
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: query_a_uri },
            position: pos,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let resp: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    let loc = match resp {
        Some(GotoDefinitionResponse::Scalar(loc)) => loc,
        _ => panic!("Expected definition location, got {:?}", resp),
    };

    assert!(
        loc.uri.to_string().contains("project-a/schema.graphql"),
        "Expected definition in project-a/schema.graphql, but got {}",
        loc.uri
    );

    // Test 2: Go to definition from WITHIN project-a/schema.graphql
    let schema_a_uri = Url::from_file_path(base_dir.join("project-a/schema.graphql")).unwrap();
    lsp_did_open(&mut service, schema_a_uri.clone(), "graphql", 1, schema_a).await;

    // Click on "User" in "type Query { user: User }" (line 0, col 19)
    let pos = Position::new(0, 19);
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: schema_a_uri.clone(),
            },
            position: pos,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let resp: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    let loc = match resp {
        Some(GotoDefinitionResponse::Scalar(loc)) => loc,
        _ => panic!(
            "Expected definition location for User in schema-a, got {:?}",
            resp
        ),
    };

    assert!(
        loc.uri.to_string().contains("project-a/schema.graphql"),
        "Expected definition for User to be in project-a/schema.graphql, but got {}",
        loc.uri
    );

    // Check that it points to the correct line (line 2 in schema_a)
    assert_eq!(
        loc.range.start.line, 2,
        "Expected User definition to start at line 2"
    );
}
