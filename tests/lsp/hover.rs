use crate::support::{
    create_doc, create_initialized_lsp_service, lsp_did_open, lsp_request_hover,
    make_temp_project_with_schema, pos, pos_for_token, with_cursor, write_project_file,
};
use tower_lsp_server::ls_types::*;

// Tests use helpers from tests/support/mod.rs to create temporary projects and
// initialize LSP services. This avoids repeating setup boilerplate.

#[tokio::test]
#[ntest::timeout(3000)]
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
                ...|UserFields
            }
        }
    "#;
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "hover.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    // 2. Request hover over 'UserFields' in the spread
    let result = lsp_request_hover(&mut service, uri.clone(), position).await;

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
#[ntest::timeout(3000)]
async fn test_hover_schema_type() {
    let schema = "type Query { users: [User!]! } type User { id: ID! username: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    // 1. Open file
    let text = r#"
        query {
            us|ers {
                id
            }
        }
    "#;
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "hover_schema.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_hover(&mut service, uri.clone(), position).await;

    let hover = result.expect("Hover should return something");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(m.value.contains("User"), "Should show type info for User");
}

#[tokio::test]
#[ntest::timeout(3000)]
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
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = std::fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    let doc = create_doc(schema_uri.as_ref(), &schema_text);
    let position = pos_for_token(&doc, &schema_text, "DocumentedType");
    let result = lsp_request_hover(&mut service, schema_uri.clone(), position).await;

    let hover = result.expect("Hover should return something");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("This is a documented type"),
        "Should show documentation description"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_schema_field() {
    let (dir, mut config) = make_temp_project_with_schema(
        "type Query { users: [User!]! } type User { id: ID! username: String! }",
        "**/*.graphql",
    );
    config = config.with_base_dir(std::fs::canonicalize(dir.path()).unwrap());
    let (mut service, _handle) = create_initialized_lsp_service(config).await;

    let text = r#"
        query {
            users {
                i|d
            }
        }
    "#;
    let (text, position) = with_cursor(text);
    let uri = write_project_file(&dir, "hover_field.graphql", &text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, &text).await;

    let result = lsp_request_hover(&mut service, uri.clone(), position).await;

    let hover = result.expect("Hover should return something");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("field `User.id`"),
        "Should show field info for User.id"
    );
    assert!(
        m.value.contains("Type: `ID!`"),
        "Should show correct field type"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_variable() {
    let (dir, mut config) = make_temp_project_with_schema(
        "type Query { users: [User!]! } type User { id: ID! username: String! }",
        "**/*.graphql",
    );
    config = config.with_base_dir(std::fs::canonicalize(dir.path()).unwrap());
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

    let hover = result.expect("Hover should return something for variable definition");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("variable $id"),
        "Should contain variable name"
    );
    assert!(
        m.value.contains("Type: `ID!`"),
        "Should contain variable type"
    );

    // Hover over its usage
    let text_with_usage = r#"
        query GetUser($id: ID!) {
            node(id: $id) {
                id
            }
        }
    "#;
    let uri_usage = write_project_file(&dir, "hover_var_usage.graphql", text_with_usage);
    lsp_did_open(
        &mut service,
        uri_usage.clone(),
        "graphql",
        1,
        text_with_usage,
    )
    .await;

    let result = lsp_request_hover(&mut service, uri_usage.clone(), pos(2, 22)).await;

    let hover = result.expect("Hover should return something for variable usage");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("variable $id"),
        "Should contain variable name"
    );
    assert!(
        m.value.contains("Type: `ID!`"),
        "Should contain variable type"
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
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

    let hover = result.expect("Hover should return something for argument 'id'");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
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
}

#[tokio::test]
#[ntest::timeout(3000)]
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

    let hover = result.expect("Hover should return something for input field 'username'");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("field `CreateUserInput.username`"),
        "Should show field info, got: {}",
        m.value
    );
    assert!(
        m.value.contains("Type: `String!`"),
        "Should show correct field type, got: {}",
        m.value
    );

    // Hover over 'age'
    let result = lsp_request_hover(&mut service, uri.clone(), pos(2, 52)).await;

    let hover = result.expect("Hover should return something for input field 'age'");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("field `CreateUserInput.age`"),
        "Should show field info, got: {}",
        m.value
    );
    assert!(
        m.value.contains("Type: `Int`"),
        "Should show correct field type, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_builtin_typename() {
    let (dir, mut config) = make_temp_project_with_schema(
        "type Query { users: [User!]! } type User { id: ID! username: String! }",
        "**/*.graphql",
    );
    config = config.with_base_dir(std::fs::canonicalize(dir.path()).unwrap());
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
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("field `User.__typename`"),
        "Should show typename field info, got: {}",
        m.value
    );
    assert!(
        m.value
            .contains("The GraphQL type name of the current selection."),
        "Should show fallback description for builtin field, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_builtin_schema_fields() {
    let (dir, mut config) = make_temp_project_with_schema(
        "type Query { users: [User!]! } type User { id: ID! username: String! }",
        "**/*.graphql",
    );
    config = config.with_base_dir(std::fs::canonicalize(dir.path()).unwrap());
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
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("field `Query.__schema`"),
        "Should describe __schema, got: {}",
        m.value
    );
    assert!(
        m.value
            .contains("Access the current schema introspection object."),
        "Should show fallback description for __schema, got: {}",
        m.value
    );

    let result = lsp_request_hover(&mut service, uri.clone(), pos(7, 14)).await;
    let hover = result.expect("hover should succeed for __type");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("field `Query.__type`"),
        "Should describe __type, got: {}",
        m.value
    );
    assert!(
        m.value.contains("Look up a type definition by its name."),
        "Should show fallback description for __type, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_enum_value() {
    let schema = r#"
        enum Status {
            "Order is being processed"
            PROCESSING
            "Order has been shipped"
            SHIPPED
        }
        type Query { orders(status: Status): [ID] }
    "#;
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let text = r#"
        query {
            orders(status: PROCESSING)
        }
    "#;
    let uri = write_project_file(&dir, "hover_enum.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_hover(&mut service, uri.clone(), pos(2, 28)).await;

    let hover = result.expect("Hover should return something for enum value");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("enum value PROCESSING"),
        "Should show enum value info, got: {}",
        m.value
    );
    assert!(
        m.value.contains("Type: `Status`"),
        "Should show enum type, got: {}",
        m.value
    );
    assert!(
        m.value.contains("Order is being processed"),
        "Should show enum value description, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_directive_enhanced() {
    let schema = r#"
        directive @custom(arg: String, required: Int!) on FIELD
        type Query { id: ID }
    "#;
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let text = r#"
        query {
            id @custom(arg: "test", required: 1)
        }
    "#;
    let uri = write_project_file(&dir, "hover_directive.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_hover(&mut service, uri.clone(), pos(2, 16)).await;

    let hover = result.expect("Hover should return something for directive");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("directive @custom"),
        "Should show directive name, got: {}",
        m.value
    );
    assert!(
        m.value.contains("Args: arg: `String`, required: `Int!`"),
        "Should show directive arguments, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_inline_fragment_type() {
    let schema = "type Query { node: Node } interface Node { id: ID! } type User implements Node { id: ID! username: String! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let text = r#"
        query {
            node {
                ... on User {
                    username
                }
            }
        }
    "#;
    let uri = write_project_file(&dir, "hover_inline.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_hover(&mut service, uri.clone(), pos(3, 24)).await;

    let hover = result.expect("Hover should return something for type User");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("type User"),
        "Should show type info for User, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_enum_value_definition() {
    let schema = r#"
        enum Status {
            "Order is being processed"
            PROCESSING
            "Order has been shipped"
            SHIPPED
        }
        type Query { id: ID }
    "#;
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let schema_path = dir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = std::fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    let result = lsp_request_hover(&mut service, schema_uri.clone(), pos(3, 14)).await;

    let hover = result.expect("Hover should return something for PROCESSING");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("PROCESSING"),
        "Should show value name, got: {}",
        m.value
    );
    assert!(
        m.value.contains("Order is being processed"),
        "Should show description, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_operation_name() {
    let schema = "type Query { user(id: ID!): User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let text = r#"
        query GetUser($id: ID!) {
            user(id: $id) {
                id
            }
        }
    "#;
    let uri = write_project_file(&dir, "hover_op.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_hover(&mut service, uri.clone(), pos(1, 17)).await;

    let hover = result.expect("Hover should return something for GetUser");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("### query GetUser"),
        "Should show operation name, got: {}",
        m.value
    );
    assert!(
        m.value.contains("Variables: $id: `ID!`"),
        "Should show variables, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_variable_default() {
    let schema = "type Query { user(name: String): User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let text = r#"
        query GetUser($name: String = "Emma") {
            user(name: $name) {
                id
            }
        }
    "#;
    let uri = write_project_file(&dir, "hover_default.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // Hover over "Emma"
    let result = lsp_request_hover(&mut service, uri.clone(), pos(1, 40)).await;

    let hover = result.expect("Hover should return something for default value");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("default value"),
        "Should show it's a default value, got: {}",
        m.value
    );
    assert!(
        m.value.contains("Type: `String`"),
        "Should show expected type, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_scalar_literal() {
    let schema = "type Query { user(id: ID!): User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let text = r#"
        query {
            user(id: "123") {
                id
            }
        }
    "#;
    let uri = write_project_file(&dir, "hover_literal.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // Hover over "123"
    let result = lsp_request_hover(&mut service, uri.clone(), pos(2, 22)).await;

    let hover = result.expect("Hover should return something for literal value");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("string value"),
        "Should show it's a string value, got: {}",
        m.value
    );
    assert!(
        m.value.contains("Expected type: `ID!`"),
        "Should show expected type, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_type_extension() {
    let schema = r#"
        type User { id: ID! }
        extend type User {
            email: String!
        }
        type Query { id: ID }
    "#;
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let schema_path = dir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = std::fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Hover over 'User' in 'extend type User'
    let result = lsp_request_hover(&mut service, schema_uri.clone(), pos(2, 22)).await;

    let hover = result.expect("Hover should return something for type extension");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("extends User"),
        "Should show it's an extension, got: {}",
        m.value
    );
    assert!(
        m.value.contains("Adds: `email: String!`"),
        "Should show added fields, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_deprecated_field() {
    let schema = r#"
        type Query {
            oldField: String @deprecated(reason: "Use newField instead")
            newField: String
        }
    "#;
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let text = r#"
        query {
            oldField
        }
    "#;
    let uri = write_project_file(&dir, "deprecated.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_hover(&mut service, uri.clone(), pos(2, 12)).await;
    let hover = result.expect("Hover should return something");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("**Deprecated:** Use newField instead"),
        "Hover should show deprecation info, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_field_arguments() {
    let schema = r#"
        type Query {
            user(id: ID!, fast: Boolean): User
        }
        type User { id: ID! }
    "#;
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let text = r#"
        query {
            user(id: "1") { id }
        }
    "#;
    let uri = write_project_file(&dir, "args.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    let result = lsp_request_hover(&mut service, uri.clone(), pos(2, 12)).await;
    let hover = result.expect("Hover should return something");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value
            .contains("field `Query.user(id: ID!, fast: Boolean)`"),
        "Hover should show field arguments in signature, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_union_members() {
    let schema = r#"
        type User { id: ID! name: String! }
        type Bot { id: ID! model: String! }
        union SearchResult = User | Bot
        type Query { search: [SearchResult!]! }
    "#;
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let schema_path = dir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = std::fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Hover over SearchResult in the schema
    let result = lsp_request_hover(&mut service, schema_uri.clone(), pos(3, 15)).await;
    let hover = result.expect("Hover should return something for union");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("- `User`"),
        "Hover should show union members, got: {}",
        m.value
    );
    assert!(
        m.value.contains("- `Bot`"),
        "Hover should show union members, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_interface_implementations() {
    let schema = r#"
        interface Node { id: ID! }
        type User implements Node { id: ID! name: String! }
        type Post implements Node { id: ID! title: String! }
        type Query { node(id: ID!): Node }
    "#;
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let schema_path = dir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = std::fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    let result = lsp_request_hover(&mut service, schema_uri.clone(), pos(1, 18)).await;
    let hover = result.expect("Hover should return something for interface");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("**Implementations:**"),
        "Hover should show interface implementations header, got: {}",
        m.value
    );
    assert!(
        m.value.contains("`User`"),
        "Hover should show User implementation, got: {}",
        m.value
    );
    assert!(
        m.value.contains("`Post`"),
        "Hover should show Post implementation, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_object_interfaces() {
    let schema = r#"
        interface Named { name: String! }
        interface Node { id: ID! }
        type User implements Named & Node { id: ID! name: String! }
        type Query { me: User }
    "#;
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let schema_path = dir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = std::fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    let result = lsp_request_hover(&mut service, schema_uri.clone(), pos(3, 13)).await;
    let hover = result.expect("Hover should return something for User");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("**Implements:**"),
        "Hover should show implemented interfaces header, got: {}",
        m.value
    );
    assert!(
        m.value.contains("`Named`"),
        "Hover should show Named interface, got: {}",
        m.value
    );
    assert!(
        m.value.contains("`Node`"),
        "Hover should show Node interface, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_input_field_description() {
    let schema = r#"
        input CreateUserInput {
            "The unique username"
            username: String!
            age: Int
        }
        type Query { createUser(input: CreateUserInput!): ID }
    "#;
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let schema_path = dir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = std::fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    let result = lsp_request_hover(&mut service, schema_uri.clone(), pos(1, 14)).await;
    let hover = result.expect("Hover should return something for CreateUserInput");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value
            .contains("- **username**: `String!` — The unique username"),
        "Hover should show input field descriptions in full type info, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_sdl_argument_name() {
    let schema = "type Query { user(id: ID!): User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let schema_path = dir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = std::fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Hover over "id" in "user(id: ID!)"
    let result = lsp_request_hover(&mut service, schema_uri.clone(), pos(0, 18)).await;
    let hover = result.expect("Hover should return something for SDL argument");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("### argument id"),
        "Hover should show argument info in SDL, got: {}",
        m.value
    );
    assert!(
        m.value.contains("Type: `ID!`"),
        "Hover should show argument type in SDL, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_sdl_argument_type() {
    let schema = "type Query { user(id: ID!): User } type User { id: ID! }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let schema_path = dir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = std::fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Hover over "ID" in "user(id: ID!)"
    let result = lsp_request_hover(&mut service, schema_uri.clone(), pos(0, 22)).await;
    let hover = result.expect("Hover should return something for SDL argument type");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("### built-in scalar ID"),
        "Hover should show type info for SDL argument type, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_sdl_directive_arg() {
    let schema = "directive @d(arg: String) on FIELD type Query { id: ID }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let schema_path = dir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = std::fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Hover over "arg" in "@d(arg: String)"
    let result = lsp_request_hover(&mut service, schema_uri.clone(), pos(0, 13)).await;
    let hover = result.expect("Hover should return something for directive argument def");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("### argument arg"),
        "Hover should show argument info for directive def, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_scalar_details() {
    let schema = "scalar JSON type Query { id: ID }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let schema_path = dir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = std::fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Hover over "JSON"
    let result = lsp_request_hover(&mut service, schema_uri.clone(), pos(0, 7)).await;
    let hover = result.expect("Hover should return something for scalar");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("### scalar JSON"),
        "Hover should show custom scalar info, got: {}",
        m.value
    );

    // Hover over "ID"
    let result = lsp_request_hover(&mut service, schema_uri.clone(), pos(0, 30)).await;
    let hover = result.expect("Hover should return something for built-in scalar");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("### built-in scalar ID"),
        "Hover should show built-in scalar info, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_implements_keyword() {
    let schema =
        "type User implements Node { id: ID! } interface Node { id: ID! } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let schema_path = dir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = std::fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Hover over "implements"
    let result = lsp_request_hover(&mut service, schema_uri.clone(), pos(0, 10)).await;
    let hover = result.expect("Hover should return something for implements");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("implements"),
        "Hover should describe implements keyword, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_multiline_description() {
    let schema = r#"
        """
        A multi-line
        description.
        """
        type User { id: ID! }
        type Query { me: User }
    "#;
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let schema_path = dir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = std::fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Hover over "User"
    let result = lsp_request_hover(&mut service, schema_uri.clone(), pos(5, 13)).await;
    let hover = result.expect("Hover should return something for multiline desc");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("A multi-line\ndescription."),
        "Hover should preserve multiline description, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_recursive_type() {
    let schema = "type User { id: ID! friend: User } type Query { me: User }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let schema_path = dir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = std::fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Hover over "User"
    let result = lsp_request_hover(&mut service, schema_uri.clone(), pos(0, 5)).await;
    let hover = result.expect("Hover should return something for recursive type");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("- **friend**: `User`"),
        "Hover should show recursive field, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_multiple_extensions() {
    let schema = r#"
        type User { id: ID! }
        extend type User { name: String! }
        extend type User { age: Int }
        type Query { me: User }
    "#;
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let schema_path = dir.path().join("schema.graphql");
    let schema_uri =
        graphox::utils::path_to_uri(std::fs::canonicalize(&schema_path).unwrap()).unwrap();
    let schema_text = std::fs::read_to_string(&schema_path).unwrap();
    lsp_did_open(&mut service, schema_uri.clone(), "graphql", 1, &schema_text).await;

    // Hover over "User" in the Query
    let result = lsp_request_hover(&mut service, schema_uri.clone(), pos(4, 25)).await;
    let hover = result.expect("Hover should return something for extended type");
    let HoverContents::Markup(m) = hover.contents else {
        panic!("expected Markup, got {:?}", hover.contents);
    };
    assert!(
        m.value.contains("- **name**: `String!`"),
        "Hover should show first extension field, got: {}",
        m.value
    );
    assert!(
        m.value.contains("- **age**: `Int`"),
        "Hover should show second extension field, got: {}",
        m.value
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_hover_invalid_syntax() {
    let schema = "type Query { id: ID }";
    let (dir, config) = make_temp_project_with_schema(schema, "**/*.graphql");
    let (mut service, _) = create_initialized_lsp_service(config).await;

    let text = "query    { user(id: ) { id } }"; // Extra spaces
    let uri = write_project_file(&dir, "invalid.graphql", text);
    lsp_did_open(&mut service, uri.clone(), "graphql", 1, text).await;

    // Hover over "user" (even with error next to it)
    let result = lsp_request_hover(&mut service, uri.clone(), pos(0, 11)).await;
    // It's okay if it returns None, as long as it doesn't crash.
    // In this case, 'user' might not even resolve if the parser fails.
    let _ = result;

    // Hover over whitespace (sufficiently far from boundaries)
    let result = lsp_request_hover(&mut service, uri.clone(), pos(0, 7)).await;
    assert!(result.is_none(), "Hover on whitespace should be None");
}
