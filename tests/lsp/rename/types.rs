use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use tower_lsp::lsp_types::*;

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_object_type_rename() {
    let schema = "type User { id: ID! } type Query { user: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor("type |User { id: ID! }");
    let uri = write_project_file(&dir, "schema.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        new_name: "Person".to_string(),
        work_done_progress_params: Default::default(),
    };
    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;
    assert!(result.is_some(), "Expected Some for type rename");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_interface_type_rename() {
    let schema = "interface Node { id: ID! } type User implements Node { id: ID! } type Query { node: Node }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor("interface |Node { id: ID! }");
    let uri = write_project_file(&dir, "schema.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        new_name: "Entity".to_string(),
        work_done_progress_params: Default::default(),
    };
    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;
    assert!(result.is_some(), "Expected Some for interface rename");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_enum_type_rename() {
    let schema = "enum Role { ADMIN USER } type Query { role: Role }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor("enum |Role { ADMIN USER }");
    let uri = write_project_file(&dir, "schema.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        new_name: "UserRole".to_string(),
        work_done_progress_params: Default::default(),
    };
    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;
    assert!(result.is_some(), "Expected Some for enum type rename");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_union_type_rename() {
    let schema = "union SearchResult = User | Post type Query { search: SearchResult } type User { id: ID! } type Post { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor("union |SearchResult = User | Post");
    let uri = write_project_file(&dir, "schema.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        new_name: "Result".to_string(),
        work_done_progress_params: Default::default(),
    };
    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;
    assert!(result.is_some(), "Expected Some for union rename");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_input_type_rename() {
    let schema = "input CreateUserInput { name: String! } type Mutation { createUser(input: CreateUserInput!): User } type User { id: ID! } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;
    let (text, position) = with_cursor("input |CreateUserInput { name: String! }");
    let uri = write_project_file(&dir, "schema.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let params = RenameParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri },
            position,
        },
        new_name: "UserInput".to_string(),
        work_done_progress_params: Default::default(),
    };
    let result: Option<WorkspaceEdit> =
        lsp_request_typed(&mut service, "textDocument/rename", &params).await;
    assert!(result.is_some(), "Expected Some for input type rename");
}
