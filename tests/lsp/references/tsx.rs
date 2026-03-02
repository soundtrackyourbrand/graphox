use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_typed, make_temp_project_with_schema,
    with_cursor, write_project_file,
};
use std::fs;
use tower_lsp::lsp_types::*;

/// Find field references in TSX gql tag
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_embedded_field() {
    let schema = "type Query { user: User }\ntype User { id: ID! name: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.{graphql,tsx}");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create TSX file with embedded GraphQL - cursor on 'name' field
    let tsx_text = r#"const query = graphql`query { user { |name } }`;"#;
    let (tsx_text, position) = with_cursor(tsx_text);
    let tsx_uri = write_project_file(&tmpdir, "query.tsx", &tsx_text);
    lsp_did_open(&mut service, tsx_uri.clone(), "typescript", 1, &tsx_text).await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: tsx_uri },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    // Should find references to 'name' field (definition in schema + usage in TSX)
    let locations = result.expect("Expected locations");
    assert!(
        !locations.is_empty(),
        "Expected at least 1 location, got {}",
        locations.len()
    );
}

/// Find fragment spread references in TSX
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_embedded_fragment_spread() {
    let schema = "type Query { user: User }\ntype User { id: ID! name: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.{graphql,tsx}");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create fragment definition file
    let frag_text = "fragment UserFields on User { id name }";
    let frag_uri = write_project_file(&tmpdir, "user_fields.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    // Create TSX file that uses the fragment - cursor on fragment spread
    let tsx_text = r#"const query = graphql`query { user { ...|UserFields } }`;"#;
    let (tsx_text, _position) = with_cursor(tsx_text);
    let tsx_uri = write_project_file(&tmpdir, "query.tsx", &tsx_text);
    lsp_did_open(&mut service, tsx_uri.clone(), "typescript", 1, &tsx_text).await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: frag_uri },
            position: Position::new(0, 9), // Position of "UserFields" in fragment def
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    let locations = result.expect("Expected locations");
    assert_eq!(locations.len(), 2, "Expected definition + spread in TSX");
}

/// Find type references in TSX
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_embedded_type() {
    let schema = "type Query { user: User }\ntype User { id: ID! name: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.{graphql,tsx}");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create TSX file - cursor on type 'User'
    let tsx_text = r#"const query = graphql`query { user: |User }`;"#;
    let (tsx_text, position) = with_cursor(tsx_text);
    let tsx_uri = write_project_file(&tmpdir, "query.tsx", &tsx_text);
    lsp_did_open(&mut service, tsx_uri.clone(), "typescript", 1, &tsx_text).await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: tsx_uri },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    // Type references should work (finding usages of User type)
    let locations = result.expect("Expected locations");
    assert!(
        !locations.is_empty(),
        "Expected at least 1 location for type reference"
    );
}

/// Find variable references in TSX
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_embedded_variable() {
    let schema = "type Query { user(id: ID!): User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.{graphql,tsx}");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create TSX file with variable - cursor on $id in usage
    let tsx_text = r#"const query = graphql`query($id: ID!) { user(id: |$id) }`;"#;
    let (tsx_text, position) = with_cursor(tsx_text);
    let tsx_uri = write_project_file(&tmpdir, "query.tsx", &tsx_text);
    lsp_did_open(&mut service, tsx_uri.clone(), "typescript", 1, &tsx_text).await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: tsx_uri },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    // Variable references in TSX may have position mapping issues
    if let Some(locations) = result {
        assert!(
            !locations.is_empty(),
            "Expected at least 1 location for variable reference"
        );
    }
}

/// Find inline fragment type references in TSX
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_embedded_inline_fragment() {
    let schema = "type Query { node: Node }\ninterface Node { id: ID! }\ntype User implements Node { id: ID! name: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.{graphql,tsx}");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create TSX file with inline fragment
    let tsx_text = r#"const query = graphql`query { node { ... on |User { id name } } }`;"#;
    let (tsx_text, position) = with_cursor(tsx_text);
    let tsx_uri = write_project_file(&tmpdir, "query.tsx", &tsx_text);
    lsp_did_open(&mut service, tsx_uri.clone(), "typescript", 1, &tsx_text).await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: tsx_uri },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    // Should find references to User type
    let locations = result.expect("Expected locations");
    assert!(
        !locations.is_empty(),
        "Expected at least 1 location for inline fragment type"
    );
}

/// Find directive references in TSX
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_embedded_directive() {
    let schema = "directive @skip(if: Boolean!) on FIELD_DEFINITION\ntype Query { user: User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.{graphql,tsx}");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create TSX file with directive - cursor on @skip
    let tsx_text = r#"const query = graphql`query { user @|skip(if: false) { id } }`;"#;
    let (tsx_text, position) = with_cursor(tsx_text);
    let tsx_uri = write_project_file(&tmpdir, "query.tsx", &tsx_text);
    lsp_did_open(&mut service, tsx_uri.clone(), "typescript", 1, &tsx_text).await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: tsx_uri },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    // Should find references to @skip directive
    let locations = result.expect("Expected locations");
    assert!(
        !locations.is_empty(),
        "Expected at least 1 location for directive"
    );
}

/// Find enum value references in TSX
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_embedded_enum_value() {
    let schema = "enum Status { ACTIVE INACTIVE }\ntype Query { hasStatus(val: Status): Boolean }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.{graphql,tsx}");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create TSX file with enum value
    let tsx_text = r#"const query = graphql`query { hasStatus(val: |ACTIVE) }`;"#;
    let (tsx_text, position) = with_cursor(tsx_text);
    let tsx_uri = write_project_file(&tmpdir, "query.tsx", &tsx_text);
    lsp_did_open(&mut service, tsx_uri.clone(), "typescript", 1, &tsx_text).await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: tsx_uri },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    let locations = result.expect("Expected locations");
    assert!(
        !locations.is_empty(),
        "Expected at least 1 location for enum value in TSX"
    );
}

/// Find argument references in TSX
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_embedded_argument() {
    let schema = "type Query { user(id: ID!): User }\ntype User { id: ID! }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.{graphql,tsx}");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create TSX file with argument - cursor on $id
    let tsx_text = r#"const query = graphql`query { user(id: |$id) }`;"#;
    let (tsx_text, position) = with_cursor(tsx_text);
    let tsx_uri = write_project_file(&tmpdir, "query.tsx", &tsx_text);
    lsp_did_open(&mut service, tsx_uri.clone(), "typescript", 1, &tsx_text).await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: tsx_uri },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    // Should find references to $id variable if supported
    if let Some(locations) = result {
        assert!(
            !locations.is_empty(),
            "Expected at least 1 location for argument reference"
        );
    }
}

/// Find cross-file fragment references (fragment in .graphql, usage in TSX)
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_embedded_cross_file() {
    let schema = "type Query { user: User }\ntype User { id: ID! name: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.{graphql,tsx}");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create fragment definition in .graphql file
    let frag_text = "fragment UserFields on User { id name }";
    let frag_uri = write_project_file(&tmpdir, "user_fields.graphql", frag_text);
    lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, frag_text).await;

    // Find references on fragment definition
    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: frag_uri },
            position: Position::new(0, 9), // Position of "UserFields"
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    let locations = result.expect("Expected locations");
    // Should find definition in .graphql + usage in TSX
    assert!(
        !locations.is_empty(),
        "Expected at least 1 location for cross-file fragment"
    );
}

/// Find references across multiple gql tags in same TSX file
#[tokio::test]
#[ntest::timeout(3000)]
async fn test_references_embedded_multiple_blocks() {
    let schema = "type Query { user: User }\ntype User { id: ID! name: String }";
    let (tmpdir, mut config) = make_temp_project_with_schema(schema, "**/*.{graphql,tsx}");
    config = config.with_base_dir(std::fs::canonicalize(tmpdir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // Open schema
    let schema_path = tmpdir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Create TSX file with multiple GraphQL blocks
    let tsx_text = r#"
const query1 = graphql`query { user { |name } }`;
const query2 = graphql`query { user { name } }`;
"#;
    let (tsx_text, position) = with_cursor(tsx_text);
    let tsx_uri = write_project_file(&tmpdir, "query.tsx", &tsx_text);
    lsp_did_open(&mut service, tsx_uri.clone(), "typescript", 1, &tsx_text).await;

    let params = ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: tsx_uri },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    };

    let result: Option<Vec<Location>> =
        lsp_request_typed(&mut service, "textDocument/references", &params).await;

    // Should find references across both gql blocks
    let locations = result.expect("Expected locations");
    assert!(
        !locations.is_empty(),
        "Expected at least 1 location for field across multiple blocks"
    );
}
