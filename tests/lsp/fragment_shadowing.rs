use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, write_project_file,
};
use graphox::Config;
use graphox::config::{CodegenConfig, GlobPattern, ProjectConfig, SchemaSource};
use std::fs;
use tempfile::TempDir;
use tower_lsp_server::ls_types::*;

#[tokio::test]
async fn test_fragment_references_respect_shadowing() {
    let dir = TempDir::new().expect("failed to create tempdir");
    let schema_text = "type Query { user: User } type User { id: ID! display: Display } type Display { id: ID! name: String }";
    let schema_path = dir.path().join("schema.graphql");
    fs::write(&schema_path, schema_text).expect("write schema");

    fs::create_dir_all(dir.path().join("packages/catalog")).unwrap();
    fs::write(dir.path().join("packages/catalog/package.json"), "{}").unwrap();
    fs::create_dir_all(dir.path().join("apps/web")).unwrap();
    fs::write(dir.path().join("apps/web/package.json"), "{}").unwrap();

    let query_a_text = "fragment ProductCard on Product @public { id } # A_DEF\nquery GetA { user { display { ...ProductCard } } } # A_USAGE";
    let query_b_text = "fragment ProductCard on Product { name } # B_DEF\nquery GetB { user { display { ...ProductCard } } } # B_USAGE";

    let query_a_uri = write_project_file(&dir, "packages/catalog/query.graphql", query_a_text);
    let query_b_uri = write_project_file(&dir, "apps/web/query.graphql", query_b_text);

    let config = Config::new_test(
        dir.path().to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single(
                    "packages/catalog/**/*.graphql".to_string(),
                ))
                .with_codegen(CodegenConfig::disabled()),
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("apps/web/**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    lsp_did_open(
        &mut service,
        query_a_uri.clone(),
        "graphql",
        1,
        query_a_text,
    )
    .await;
    lsp_did_open(
        &mut service,
        query_b_uri.clone(),
        "graphql",
        1,
        query_b_text,
    )
    .await;

    // 1. Verify Go to Definition
    let params_def = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_b_uri.clone(),
            },
            position: Position::new(1, 35),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };
    let result_def: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params_def).await;
    let loc_def = match result_def.expect("Should have definition") {
        GotoDefinitionResponse::Scalar(l) => l,
        _ => panic!("Expected scalar location"),
    };
    assert!(
        loc_def.uri.as_str().contains("apps/web"),
        "Definition for B usage should be in B, got {}",
        loc_def.uri
    );

    // 2. Verify References
    let params_ref = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_a_uri.clone(),
            },
            position: Position::new(0, 9),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result_ref: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params_ref).await;
    let locations = result_ref.expect("Should have references");
    for loc in &locations {
        assert!(
            loc.uri.as_str().contains("packages/catalog"),
            "Reference in {} should be in Project A",
            loc.uri
        );
    }
    assert_eq!(
        locations.len(),
        2,
        "Should have 2 references in Project A (def + usage)"
    );

    // 3. Verify Rename
    let params_rename = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_a_uri.clone(),
            },
            position: Position::new(0, 9),
        },
        new_name: "RenamedProductCard".to_string(),
        work_done_progress_params: Default::default(),
    };

    let result_rename: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params_rename).await;
    let edit = result_rename.expect("Should have rename edits");
    let changes = edit.changes.expect("Should have changes map");

    assert_eq!(
        changes.len(),
        1,
        "Rename should only affect 1 file (Project A's query.graphql)"
    );
    assert!(
        changes.contains_key(&query_a_uri),
        "Should contain Project A's URI"
    );
    assert!(
        !changes.contains_key(&query_b_uri),
        "Should NOT contain Project B's URI"
    );
}
