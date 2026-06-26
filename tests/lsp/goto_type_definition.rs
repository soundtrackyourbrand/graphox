use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use std::fs;
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_type_definition_operation() {
    let schema = "type Query { user: User }\ntype User { id: ID! name: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());

    // Enable automatic codegen mock (we'll manually create the codegen file for the test)
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Create a query file
    let (query_text, position) = with_cursor("query Get|User { user { id name } }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);

    // Create the expected codegen file
    let codegen_text = "export type GetUserDocument = { __typename?: 'Query', user: { __typename?: 'User', id: string, name: string } };\nexport type GetUserQuery = { user: { id: string, name: string } };";
    // Codegen replaces the extension: query.graphql -> query.codegen.ts.
    let codegen_path = tmpdir.path().join("query.codegen.ts");
    fs::write(&codegen_path, codegen_text).unwrap();
    let codegen_uri = Url::from_file_path(fs::canonicalize(&codegen_path).unwrap()).unwrap();

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

    // Use "textDocument/typeDefinition"
    let result: Option<GotoDefinitionResponse> =
        lsp_request_typed(&mut service, "textDocument/typeDefinition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, codegen_uri);
        // Should point to "export type GetUserQuery"
        assert_eq!(loc.range.start.line, 1);
    } else {
        panic!("Expected type definition of GetUser, got {:?}", result);
    }
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_type_definition_fragment_spread() {
    let schema = "type Query { user: User }\ntype User { id: ID! name: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Create fragment file
    let frag_text = "fragment UserFields on User { id name }";
    let frag_uri = write_project_file(&tmpdir, "fragment.graphql", frag_text);

    // Create fragment codegen - matching current default (no suffix)
    let frag_codegen_text = "export type UserFields = { id: string, name: string };";
    let frag_codegen_path = tmpdir.path().join("fragment.codegen.ts");
    fs::write(&frag_codegen_path, frag_codegen_text).unwrap();
    let frag_codegen_uri =
        Url::from_file_path(fs::canonicalize(&frag_codegen_path).unwrap()).unwrap();

    // Create query file with spread
    let (query_text, position) = with_cursor("query GetUser { user { ...UserF|ields } }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);

    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;
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
        lsp_request_typed(&mut service, "textDocument/typeDefinition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, frag_codegen_uri);
        assert_eq!(loc.range.start.line, 0);
    } else {
        panic!(
            "Expected type definition of UserFields fragment, got {:?}",
            result
        );
    }
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_type_definition_nested_field() {
    let schema = "type Query { user: User }\n\
         type User { id: ID! address: Address }\n\
         type Address { city: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Cursor on a nested field two levels deep: user -> address.
    let (query_text, position) = with_cursor("query GetUser { user { addr|ess { city } } }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);

    // Generated interface with the field emitted as a nested inline property.
    let codegen_text = "export interface GetUserQuery {\n  \
         user?: {\n    id: string;\n    \
         address?: {\n      city?: string | null;\n    } | null;\n  } | null;\n}\n";
    let codegen_path = tmpdir.path().join("query.codegen.ts");
    fs::write(&codegen_path, codegen_text).unwrap();
    let codegen_uri = Url::from_file_path(fs::canonicalize(&codegen_path).unwrap()).unwrap();

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
        lsp_request_typed(&mut service, "textDocument/typeDefinition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, codegen_uri);
        // Should land on the nested `address?:` property (line index 3).
        assert_eq!(loc.range.start.line, 3);
        let line = codegen_text.lines().nth(3).unwrap();
        assert!(line.trim_start().starts_with("address?:"));
    } else {
        panic!("Expected type definition of nested field address, got {:?}", result);
    }
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_goto_type_definition_inline_fragment_field() {
    let schema = "type Query { node: Node }\n\
         union Node = User | Device\n\
         type User { id: ID! name: String }\n\
         type Device { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.graphql");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());

    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Cursor on `name`, which lives inside `... on User` (a union member in codegen).
    let (query_text, position) =
        with_cursor("query GetNode { node { ... on User { na|me } } }");
    let query_uri = write_project_file(&tmpdir, "query.graphql", &query_text);

    // `node` is a discriminated union; `name` only exists in the User member.
    let codegen_text = "export interface GetNodeQuery {\n  \
         node?:\n    | {\n      __typename: \"Device\";\n      id: string;\n    }\n    \
         | {\n      __typename: \"User\";\n      id: string;\n      name?: string | null;\n    } | null;\n}\n";
    let codegen_path = tmpdir.path().join("query.codegen.ts");
    fs::write(&codegen_path, codegen_text).unwrap();
    let codegen_uri = Url::from_file_path(fs::canonicalize(&codegen_path).unwrap()).unwrap();

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
        lsp_request_typed(&mut service, "textDocument/typeDefinition", &params).await;

    if let Some(GotoDefinitionResponse::Scalar(loc)) = result {
        assert_eq!(loc.uri, codegen_uri);
        // Must land on `name?:` inside the User member, not the Device member.
        let line = codegen_text.lines().nth(loc.range.start.line as usize).unwrap();
        assert!(
            line.trim_start().starts_with("name?:"),
            "expected to land on `name?:`, got line: {line:?}"
        );
    } else {
        panic!("Expected type definition of inline-fragment field name, got {:?}", result);
    }
}
