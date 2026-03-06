use graphox::features::diagnostics::DocumentDiagnostics;

use crate::support::{assert_diagnostic_with_message, assert_diagnostics_count, create_doc};

#[test]
#[ntest::timeout(300)]
fn test_enum_value_validation() {
    let schema_content = r#"
        enum Status {
            ACTIVE
            INACTIVE
            PENDING
        }
        type Query {
            getStatus(id: ID!): Status
        }
    "#;
    let schema = apollo_compiler::Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();
    let query = "query { getStatus(id: \"1\") }";
    let doc = create_doc("file:///test.graphql", query);
    let diags = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    assert_diagnostics_count(&diags, 0);
}

#[test]
#[ntest::timeout(300)]
fn test_invalid_enum_value() {
    let schema_content = r#"
        enum Status {
            ACTIVE
            INACTIVE
        }
        type Query {
            getStatus(id: ID!): Status
        }
    "#;
    let schema = apollo_compiler::Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();
    let query = "query { getStatus(id: \"1\", status: INVALID) }";
    let doc = create_doc("file:///test.graphql", query);
    let diags = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    assert_diagnostics_count(&diags, 1);
    assert_diagnostic_with_message(&diags, "INVALID");
}

#[test]
#[ntest::timeout(300)]
fn test_object_value_validation() {
    let schema_content = r#"
        input CreateUserInput {
            username: String!
            email: String
        }
        type Query {
            createUser(input: CreateUserInput): String
        }
    "#;
    let schema = apollo_compiler::Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();
    let query = r#"query { createUser(input: { username: "test", email: "test@test.com" }) }"#;
    let doc = create_doc("file:///test.graphql", query);
    let diags = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    assert_diagnostics_count(&diags, 0);
}

#[test]
#[ntest::timeout(300)]
fn test_object_value_missing_required_field() {
    let schema_content = r#"
        input CreateUserInput {
            username: String!
            email: String
        }
        type Query {
            createUser(input: CreateUserInput): String
        }
    "#;
    let schema = apollo_compiler::Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();
    let query = r#"query { createUser(input: { email: "test@test.com" }) }"#;
    let doc = create_doc("file:///test.graphql", query);
    let diags = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    assert_diagnostics_count(&diags, 1);
    assert_diagnostic_with_message(&diags, "username");
}

#[test]
#[ntest::timeout(300)]
fn test_list_value_validation() {
    let schema_content = r#"
        type Query {
            tags: [String]
            ids: [ID!]
        }
    "#;
    let schema = apollo_compiler::Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();
    let query = "query { tags }";
    let doc = create_doc("file:///test.graphql", query);
    let diags = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    assert_diagnostics_count(&diags, 0);
}

#[test]
#[ntest::timeout(300)]
fn test_variable_coercion() {
    let schema_content = r#"
        type Query {
            user(id: ID!): String
        }
    "#;
    let schema = apollo_compiler::Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();
    let query = r#"query($id: ID!) { user(id: $id) }"#;
    let doc = create_doc("file:///test.graphql", query);
    let diags = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    assert_diagnostics_count(&diags, 0);
}

#[test]
#[ntest::timeout(300)]
fn test_input_object_unknown_fields() {
    let schema_content = r#"
        input CreateUserInput {
            username: String!
        }
        type Query {
            createUser(input: CreateUserInput): String
        }
    "#;
    let schema = apollo_compiler::Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();
    let query = r#"query { createUser(input: { username: "test", unknownField: "value" }) }"#;
    let doc = create_doc("file:///test.graphql", query);
    let diags = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    assert_diagnostics_count(&diags, 1);
    assert_diagnostic_with_message(&diags, "unknownField");
}
