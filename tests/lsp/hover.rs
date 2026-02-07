use crate::support::{
    create_initialized_lsp_service, lsp_did_open, lsp_request_hover, make_temp_project_with_schema,
    pos, write_project_file,
};
use tower_lsp::lsp_types::*;

// Tests use helpers from tests/support/mod.rs to create temporary projects and
// initialize LSP services. This avoids repeating setup boilerplate.

#[tokio::test]
async fn test_hover_fragment_spread() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Open file with fragment and spread
    let text = r#"
        fragment UserFields on User {
            id
            username
        }

        query {
            users {
                ...UserFields
            }
        }
    "#;
    let uri = write_project_file(&dir, "hover.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // 2. Request hover over 'UserFields' in the spread
    let result = lsp_request_hover(&mut service, uri.clone(), pos(8, 20)).await;

    assert!(result.is_some(), "Hover should return something");
    let hover = result.unwrap();
    match hover.contents {
        HoverContents::Markup(m) => {
            assert!(
                m.value.contains("UserFields"),
                "Hover content should contain fragment name"
            );
            assert!(
                m.value.contains("id"),
                "Hover content should contain fragment fields"
            );
            assert!(
                m.value.contains("username"),
                "Hover content should contain fragment fields"
            );
        }
        _ => panic!("Expected Markup hover contents"),
    }
}

#[tokio::test]
async fn test_hover_schema_type() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Open file
    let text = r#"
        query {
            users {
                id
            }
        }
    "#;
    let uri = write_project_file(&dir, "hover_schema.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_hover(&mut service, uri.clone(), pos(2, 13)).await;

    assert!(result.is_some(), "Hover should return something");
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(m.value.contains("User"), "Should show type info for User");
    } else {
        panic!("Expected Markup contents");
    }
}

#[tokio::test]
async fn test_hover_graphql_description() {
    let schema = r#"
        "This is a documented type"
        type DocumentedType {
            id: ID!
        }
        type Query { someField(arg: DocumentedType): ID }
    "#;

    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = r#"
        query {
            someField(arg: { id: "1" }): ID
        }
    "#;
    let uri = write_project_file(&dir, "hover_desc.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let schema_path = dir.path().join("schema.graphql");
    let schema_uri = Url::from_file_path(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = std::fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    let result = lsp_request_hover(&mut service, schema_uri.clone(), pos(2, 15)).await;

    assert!(result.is_some(), "Hover should return something");
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(
            m.value.contains("This is a documented type"),
            "Should show documentation description"
        );
    } else {
        panic!("Expected Markup contents");
    }
}

#[tokio::test]
async fn test_hover_schema_field() {
    let (dir, mut config) = make_temp_project_with_schema(
        "type Query { users: [User!]! } type User { id: ID! username: String! }",
        "**/*.graphql",
    );
    config.base_dir = dir.path().to_path_buf();
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = r#"
        query {
            users {
                id
            }
        }
    "#;
    let uri = write_project_file(&dir, "hover_field.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_hover(&mut service, uri.clone(), pos(3, 17)).await;

    assert!(result.is_some(), "Hover should return something");
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(
            m.value.contains("field User.id"),
            "Should show field info for User.id"
        );
        assert!(
            m.value.contains("Type: `ID!`"),
            "Should show correct field type"
        );
    } else {
        panic!("Expected Markup contents");
    }
}

#[tokio::test]
async fn test_hover_variable() {
    let (dir, mut config) = make_temp_project_with_schema(
        "type Query { users: [User!]! } type User { id: ID! username: String! }",
        "**/*.graphql",
    );
    config.base_dir = dir.path().to_path_buf();
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Open file with variable
    let text = r#"
        query GetUser($id: ID!) {
            users {
                id
            }
        }
    "#;
    let uri = write_project_file(&dir, "hover_var.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // Hover over '$id' in the variable definition
    let result = lsp_request_hover(&mut service, uri.clone(), pos(1, 22)).await;

    assert!(
        result.is_some(),
        "Hover should return something for variable definition"
    );
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(
            m.value.contains("variable $id"),
            "Should contain variable name"
        );
        assert!(
            m.value.contains("Type: `ID!`"),
            "Should contain variable type"
        );
    } else {
        panic!("Expected Markup contents");
    }

    // Hover over its usage
    let text_with_usage = r#"
        query GetUser($id: ID!) {
            node(id: $id) {
                id
            }
        }
    "#;
    let uri_usage = write_project_file(&dir, "hover_var_usage.graphql", text_with_usage);
    lsp_did_open(&mut service, uri_usage.clone(), "graphql", 1, text_with_usage).await;

    let result = lsp_request_hover(&mut service, uri_usage.clone(), pos(2, 22)).await;

    assert!(
        result.is_some(),
        "Hover should return something for variable usage"
    );
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(
            m.value.contains("variable $id"),
            "Should contain variable name"
        );
        assert!(
            m.value.contains("Type: `ID!`"),
            "Should contain variable type"
        );
    } else {
        panic!("Expected Markup contents");
    }
}

#[tokio::test]
async fn test_hover_argument() {
    let (dir, config) = make_temp_project_with_schema(
        "type Query { user(id: ID!): User } type User { id: ID! username: String! }",
        "**/*.graphql",
    );
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let text = r#"
        query {
            user(id: "1") {
                id
            }
        }
    "#;
    let uri = write_project_file(&dir, "hover_arg.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_hover(&mut service, uri.clone(), pos(2, 17)).await;

    assert!(
        result.is_some(),
        "Hover should return something for argument 'id'"
    );
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(
            m.value.contains("argument id"),
            "Should show argument info for 'id', got: {}",
            m.value
        );
        assert!(
            m.value.contains("Type: `ID!`"),
            "Should show correct argument type, got: {}",
            m.value
        );
    } else {
        panic!("Expected Markup contents");
    }
}

#[tokio::test]
async fn test_hover_input_object_field() {
    let (dir, config) = make_temp_project_with_schema(
        "type Query { createUser(input: CreateUserInput!): User } \
         input CreateUserInput { username: String! age: Int } \
         type User { id: ID! username: String! }",
        "**/*.graphql",
    );

    let (mut service, _) = create_initialized_lsp_service(config).await;

    let text = r#"
        query {
            createUser(input: { username: "emma", age: 25 }) {
                id
            }
        }
    "#;
    let uri = write_project_file(&dir, "hover_input.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // Hover over 'username' in the input object
    let result = lsp_request_hover(&mut service, uri.clone(), pos(2, 35)).await;

    assert!(
        result.is_some(),
        "Hover should return something for input field 'username'"
    );
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(
            m.value.contains("field CreateUserInput.username"),
            "Should show field info, got: {}",
            m.value
        );
        assert!(
            m.value.contains("Type: `String!`"),
            "Should show correct field type, got: {}",
            m.value
        );
    } else {
        panic!("Expected Markup contents");
    }

    // Hover over 'age'
    let result = lsp_request_hover(&mut service, uri.clone(), pos(2, 52)).await;

    assert!(
        result.is_some(),
        "Hover should return something for input field 'age'"
    );
    if let HoverContents::Markup(m) = result.unwrap().contents {
        assert!(
            m.value.contains("field CreateUserInput.age"),
            "Should show field info, got: {}",
            m.value
        );
        assert!(
            m.value.contains("Type: `Int`"),
            "Should show correct field type, got: {}",
            m.value
        );
    } else {
        panic!("Expected Markup contents");
    }
}

#[tokio::test]
async fn test_hover_builtin_typename() {
    let (dir, mut config) = make_temp_project_with_schema(
        "type Query { users: [User!]! } type User { id: ID! username: String! }",
        "**/*.graphql",
    );
    config.base_dir = dir.path().to_path_buf();
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = r#"
        query {
            users {
                __typename
            }
        }
    "#;
    let uri = write_project_file(&dir, "hover_typename.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_hover(&mut service, uri.clone(), pos(3, 20)).await;

    let hover = result.expect("hover should succeed");
    if let HoverContents::Markup(m) = hover.contents {
        assert!(
            m.value.contains("field User.__typename"),
            "Should show typename field info, got: {}",
            m.value
        );
        assert!(
            m.value
                .contains("The GraphQL type name of the current selection."),
            "Should show fallback description for builtin field, got: {}",
            m.value
        );
    } else {
        panic!("Expected Markup contents");
    }
}

#[tokio::test]
async fn test_hover_builtin_schema_fields() {
    let (dir, mut config) = make_temp_project_with_schema(
        "type Query { users: [User!]! } type User { id: ID! username: String! }",
        "**/*.graphql",
    );
    config.base_dir = dir.path().to_path_buf();
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = r#"
        query {
            __schema {
                types {
                    name
                }
            }
            __type(name: "User") {
                name
            }
        }
    "#;
    let uri = write_project_file(&dir, "hover_schema_fields.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_hover(&mut service, uri.clone(), pos(2, 14)).await;
    let hover = result.expect("hover should succeed for __schema");
    if let HoverContents::Markup(m) = hover.contents {
        assert!(
            m.value.contains("field Query.__schema"),
            "Should describe __schema, got: {}",
            m.value
        );
        assert!(
            m.value
                .contains("Access the current schema introspection object."),
            "Should show fallback description for __schema, got: {}",
            m.value
        );
    } else {
        panic!("Expected Markup contents");
    }

    let result = lsp_request_hover(&mut service, uri.clone(), pos(7, 14)).await;
    let hover = result.expect("hover should succeed for __type");
    if let HoverContents::Markup(m) = hover.contents {
        assert!(
            m.value.contains("field Query.__type"),
            "Should describe __type, got: {}",
            m.value
        );
        assert!(
            m.value.contains("Look up a type definition by its name."),
            "Should show fallback description for __type, got: {}",
            m.value
        );
    } else {
        panic!("Expected Markup contents");
    }
}

