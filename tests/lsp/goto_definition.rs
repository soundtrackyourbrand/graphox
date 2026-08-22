use crate::support::{
    create_doc, create_initialized_lsp_service, lsp_did_open, lsp_request_typed,
    make_temp_project_with_schema, with_cursor, write_project_file,
};
use graphox_core::config::{GlobPattern, ProjectConfig, SchemaSource};
use graphox_core::{CodegenConfig, Config};
use std::fs;
use tempfile::TempDir;
use tower_lsp_server::ls_types::*;

#[tokio::test]
#[ntest::timeout(500)]
async fn test_goto_definition_type_vs_fragment_collision() {
    let schema = "type Query { user: User }\ntype User { id: ID! }\ntype Displayable { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    // keep base_dir consistent
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Uri::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    let schema_doc = create_doc(schema_uri.as_str(), &schema_text);

    // Open schema
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create a fragment where fragment name = type name
    let frag_text = "fragment Displayable on |Displayable { id }";
    let (frag_text, position) = with_cursor(frag_text);
    let frag_uri = write_project_file(&tmpdir, "frag.graphql", &frag_text);

    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, &frag_text).await;

    let frag_doc = create_doc(frag_uri.as_str(), &frag_text);

    // 1. Trigger Go to Definition on "Displayable" in type condition position
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: frag_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        assert_eq!(
            loc.range,
            crate::support::range_for_token_at_index(&schema_doc, &schema_text, "Displayable", 0)
        );
    } else {
        panic!("Expected definition of Displayable type, got {:?}", result);
    }

    // 2. Trigger Go to Definition on fragment spread
    let query_text = "query GetUser { user { ...|Displayable } }";
    let (query_text, position) = with_cursor(query_text);
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);

    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(ref loc)) = result {
        assert_eq!(loc.uri, frag_uri);
        assert_eq!(
            loc.range,
            crate::support::range_for_token_at_index(&frag_doc, &frag_text, "Displayable", 0)
        );
    } else {
        panic!(
            "Expected definition of Displayable fragment, got {:?}",
            result
        );
    }
}

#[tokio::test]
#[ntest::timeout(500)]
async fn test_goto_definition_directive() {
    let schema = "directive @customDirective(arg: String) on FIELD\ntype Query { id: ID }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Uri::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    let schema_doc = create_doc(schema_uri.as_str(), &schema_text);

    // Open schema
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    let (query_text, position) = with_cursor("query { id @custom|Directive(arg: \"test\") }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);

    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        assert_eq!(
            loc.range,
            crate::support::range_for_token(&schema_doc, &schema_text, "customDirective")
        );
    } else {
        panic!("Expected definition of customDirective, got {:?}", result);
    }
}

#[tokio::test]
#[ntest::timeout(500)]
async fn test_goto_definition_directive_on_fragment_spread() {
    let schema = "directive @customDirective(if: Boolean!) on FRAGMENT_SPREAD\n\
type Query { user: User }\n\
type User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Uri::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    let schema_doc = create_doc(schema_uri.as_str(), &schema_text);

    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    let frag_text = "fragment UserFields on User { id }";
    let frag_uri = write_project_file(&tmpdir, "frag.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri, "graphql", 1, frag_text).await;

    let (query_text, position) =
        with_cursor("query { user { ...UserFields @custom|Directive(if: true) } }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        assert_eq!(
            loc.range,
            crate::support::range_for_token(&schema_doc, &schema_text, "customDirective")
        );
    } else {
        panic!(
            "Expected definition of customDirective from fragment spread directive, got {:?}",
            result
        );
    }
}

#[tokio::test]
#[ntest::timeout(500)]
async fn test_goto_definition_enum_value() {
    let schema = "enum OrderStatus { PENDING ACTIVE COMPLETED }\ntype Query { users(status: OrderStatus): [User] }\ntype User { id: ID }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Uri::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    let schema_doc = create_doc(schema_uri.as_str(), &schema_text);

    // Open schema
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create query with enum value
    let (query_text, position) = with_cursor("query GetUsers { users(status: ACT|IVE) { id } }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);

    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        assert_eq!(
            loc.range,
            crate::support::range_for_token(&schema_doc, &schema_text, "ACTIVE")
        );
    } else {
        panic!("Expected definition of ACTIVE enum value, got {:?}", result);
    }
}

#[tokio::test]
#[ntest::timeout(500)]
async fn test_goto_definition_variable_in_argument() {
    let (tmpdir, mut config) = make_temp_project_with_schema(
        "type Query { user(id: ID!): User }\ntype User { id: ID! name: String }",
        "**/*.graphql",
    );
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Uri::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    let schema_doc = create_doc(schema_uri.as_str(), &schema_text);

    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    let query_text = "query GetUser($id: ID!) { user(|id: $|id) { name } }";
    let (query_text, positions) = crate::support::with_cursors(query_text);
    let position2 = positions[0]; // on 'id' in user(id: ...)
    let position1 = positions[1]; // on '$id' in user(..., $id)

    let query_doc = create_doc("file:///query.graphql", &query_text);

    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    // 1. Trigger Go to Definition on "$id" in "id: $id"
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: position1,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, query_uri);
        assert_eq!(
            loc.range,
            crate::support::range_for_token_at_index(&query_doc, &query_text, "$id", 0)
        );
    } else {
        panic!("Expected definition of $id variable, got {:?}", result);
    }

    // 2. Trigger Go to Definition on "id" (argument name) in "user(id: $id)"
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: position2,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        assert_eq!(
            loc.range,
            crate::support::range_for_token_at_index(&schema_doc, &schema_text, "id", 0)
        );
    } else {
        panic!(
            "Expected definition of id argument in schema, got {:?}",
            result
        );
    }
}

#[tokio::test]
#[ntest::timeout(500)]
async fn test_goto_definition_inline_fragment_type() {
    let schema = "type Query { user: User }\ntype User { id: ID! name: String }\ntype Admin { id: ID! privileges: [String] }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Uri::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    let _schema_doc = create_doc(schema_uri.as_str(), &schema_text);

    // Open schema
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create query with inline fragment
    let (query_text, position) = with_cursor("query GetUsers { user { ... on Us|er { name } } }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);

    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    // Trigger Go to Definition on "User" in "... on User"
    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        // Should navigate to the User type definition in schema
        assert!(
            loc.range.start.line <= 2,
            "Expected definition in schema file, got range {:?}",
            loc.range
        );
    } else {
        panic!("Expected definition of User type, got {:?}", result);
    }
}

#[tokio::test]
#[ntest::timeout(500)]
async fn test_goto_definition_input_object_field() {
    let schema = "input CreateUserInput { id: ID! name: String }\ntype Mutation { createUser(input: CreateUserInput): ID }\ntype Query { dummy: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Uri::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    let _schema_doc = create_doc(schema_uri.as_str(), &schema_text);

    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    let (query_text, position) =
        with_cursor("mutation { createUser(input: { i|d: \"1\", name: \"test\" }) }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;
    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        // Should find "id" in CreateUserInput
    } else {
        panic!("Expected definition of input field id, got {:?}", result);
    }
}

#[tokio::test]
#[ntest::timeout(500)]
async fn test_goto_definition_nested_enum_value() {
    let schema = "enum OrderStatus { PENDING ACTIVE COMPLETED }\ninput OrderFilter { status: OrderStatus }\ntype Query { orders(filter: OrderFilter): [ID] }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Uri::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    let schema_doc = create_doc(schema_uri.as_str(), &schema_text);

    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    let (query_text, position) = with_cursor("query { orders(filter: { status: ACT|IVE }) }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, &query_text).await;

    let params = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params).await;
    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, schema_uri);
        assert_eq!(
            loc.range,
            crate::support::range_for_token(&schema_doc, &schema_text, "ACTIVE")
        );
    } else {
        panic!("Expected definition of ACTIVE enum value, got {:?}", result);
    }
}

#[tokio::test]
#[ntest::timeout(500)]
async fn test_goto_definition_to_schema_file_outside_include() {
    let dir = TempDir::new().expect("failed to create tempdir");

    // 1. Create a schema file OUTSIDE the include root
    fs::create_dir_all(dir.path().join("schema")).unwrap();
    let schema_text = "type Query { user: User }\ntype User {\n  id: ID!\n  name: String\n}";
    let schema_path = dir.path().join("schema/schema.graphql");
    fs::write(&schema_path, schema_text).expect("write schema");

    // 2. Create an operation file INSIDE the include root
    fs::create_dir_all(dir.path().join("src")).unwrap();
    let query_text = "query GetUser { user { id name } }";
    let query_uri = write_project_file(&dir, "src/query.graphql", query_text);

    let config = Config::new_test(
        dir.path().to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema/schema.graphql".to_string()))
                .with_include(GlobPattern::Single("src/**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(false)
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open operation file
    lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    // 3. Test Go to Definition on 'user' field
    let params_field = GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: query_uri.clone(),
            },
            position: Position::new(0, 16),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let result_field: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/definition", &params_field).await;

    // EXPECTED FAILURE: Currently it might return None if schema file is not indexed as document
    let loc_field = match result_field {
        Some(GotoDefinitionResponse::Scalar(l)) => l,
        _ => panic!(
            "REPRO: Should have definition for 'user' field in schema/schema.graphql. Got: {:?}",
            result_field
        ),
    };

    assert!(
        loc_field.uri.as_str().contains("schema.graphql"),
        "Definition should be in schema.graphql, got {}",
        loc_field.uri
    );
}
