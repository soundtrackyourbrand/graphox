use graphql_rust::{
    Backend, Config,
    config::{GlobPattern, ProjectConfig, SchemaSource},
};
use std::fs;
use tempfile::tempdir;
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;
use tower_service::Service;

#[tokio::test]
async fn test_signature_help() {
    let dir = tempdir().unwrap();
    let base_dir = fs::canonicalize(dir.path()).unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { me(id: ID, name: String): String }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();
    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    let query_path = base_dir.join("query.graphql");
    let query_text = "query { me(id: \"123\", ) }";
    fs::write(&query_path, query_text).unwrap();
    let query_uri = Url::from_file_path(&query_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: query_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: query_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Position after "id: \"123\", "
    let position = Position::new(0, 21);
    let params = SignatureHelpParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        context: None,
    };

    let request = Request::build("textDocument/signatureHelp")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<SignatureHelp> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let help = result.expect("Expected SignatureHelp");
    assert_eq!(help.signatures.len(), 1);
    assert_eq!(help.signatures[0].label, "me(id: ID, name: String)");
    assert_eq!(help.active_parameter, Some(1)); // Should be on "name"
}

#[tokio::test]
async fn test_signature_help_tsx() {
    let dir = tempdir().unwrap();
    let base_dir = fs::canonicalize(dir.path()).unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { me(id: ID, name: String): String }",
    )
    .unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.tsx".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: None,
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();
    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    let tsx_path = base_dir.join("Component.tsx");
    let tsx_text = "const q = gql`query { me(id: \"123\", ) }`;";
    fs::write(&tsx_path, tsx_text).unwrap();
    let tsx_uri = Url::from_file_path(&tsx_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: tsx_uri.clone(),
                            language_id: "typescriptreact".to_string(),
                            version: 1,
                            text: tsx_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Position after "id: \"123\", " in TSX
    // const q = gql`query { me(id: "123", ) }`;
    // 012345678901234567890123456789012345
    //              ^ (query starts at 15)
    // 15 + 21 = 36
    let position = Position::new(0, 36);
    let params = SignatureHelpParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: tsx_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        context: None,
    };

    let request = Request::build("textDocument/signatureHelp")
        .id(1)
        .params(serde_json::to_value(&params).unwrap())
        .finish();
    let response = service.call(request).await.unwrap().unwrap();
    let result: Option<SignatureHelp> =
        serde_json::from_value(response.result().unwrap().clone()).unwrap();

    let help = result.expect("Expected SignatureHelp in TSX");
    assert_eq!(help.signatures[0].label, "me(id: ID, name: String)");
    assert_eq!(help.active_parameter, Some(1));
}
