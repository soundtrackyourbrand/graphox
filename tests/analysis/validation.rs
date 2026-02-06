use apollo_compiler::Schema;
use graphql_rust::DocumentState;
use graphql_rust::features::completion::FragmentCompletionInfo;
use std::sync::OnceLock;
use tower_lsp::lsp_types::*;

// Shared schema for tests
static SCHEMA: OnceLock<Schema> = OnceLock::new();
static VALID_SCHEMA: OnceLock<apollo_compiler::validation::Valid<Schema>> = OnceLock::new();

fn get_schema() -> &'static Schema {
    SCHEMA.get_or_init(|| {
        let schema_content = std::fs::read_to_string("tests/fixtures/simple_schema.graphql")
            .expect("Failed to read schema file");
        Schema::parse(&schema_content, "schema.graphql").expect("Failed to parse schema")
    })
}

fn get_valid_schema() -> &'static apollo_compiler::validation::Valid<Schema> {
    VALID_SCHEMA.get_or_init(|| {
        get_schema()
            .clone()
            .validate()
            .expect("Schema validation failed")
    })
}

fn create_doc(uri_str: &str, text: &str) -> DocumentState {
    let uri = Url::parse(uri_str).unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();
    DocumentState::new(uri, text, parser)
}

#[test]
#[ntest::timeout(100)]
fn test_validation_valid_query() {
    let text = r#"
        query GetUser {
            users {
                id
                username
                email
            }
        }
    "#;
    let doc = create_doc("file:///valid.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);

    assert!(
        diagnostics.is_empty(),
        "Expected no diagnostics, got: {:?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_missing_field() {
    let text = r#"
        query GetUser {
            users {
                id
                nonExistentField
            }
        }
    "#;
    let doc = create_doc("file:///missing.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);

    let error = diagnostics.iter().find(|d| d.message.contains("not found"));
    assert!(error.is_some(), "Expected 'not found' error");
    assert_eq!(error.unwrap().severity, Some(DiagnosticSeverity::ERROR));
    assert!(error.unwrap().message.contains("nonExistentField"));
    assert!(error.unwrap().message.contains("User")); // Should mention parent type
}

#[test]
#[ntest::timeout(100)]
fn test_validation_deprecated_field() {
    let text = r#"
        query GetUser {
            users {
                id
                oldField
            }
        }
    "#;
    let doc = create_doc("file:///deprecated.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);

    let warning = diagnostics
        .iter()
        .find(|d| d.message.contains("deprecated"));
    assert!(warning.is_some(), "Expected 'deprecated' warning");
    assert_eq!(warning.unwrap().severity, Some(DiagnosticSeverity::WARNING));
    assert!(warning.unwrap().message.contains("oldField"));
    assert!(
        warning.unwrap().message.contains("Use username instead"),
        "Message should contain reason: {}",
        warning.unwrap().message
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_nested_missing_field() {
    let text = r#"
        query GetPosts {
            posts {
                id
                author {
                    username
                    missingInAuthor
                }
            }
        }
    "#;
    let doc = create_doc("file:///nested.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);

    let error = diagnostics
        .iter()
        .find(|d| d.message.contains("missingInAuthor"));
    assert!(error.is_some(), "Expected nested missing field error");
    assert!(error.unwrap().message.contains("User")); // Author is User
}

#[test]
#[ntest::timeout(100)]
fn test_validation_fragment() {
    let text = r#"
        fragment UserFrag on User {
            id
            missingInFragment
        }
    "#;
    let doc = create_doc("file:///fragment.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);

    let error = diagnostics
        .iter()
        .find(|d| d.message.contains("missingInFragment"));
    assert!(error.is_some(), "Expected error in fragment");
    assert!(error.unwrap().message.contains("User"));
}

#[test]
#[ntest::timeout(100)]
fn test_validation_inline_fragment() {
    let text = r#"
        query {
            users {
                ... on User {
                    id
                    nonExistentOnUser
                }
            }
        }
    "#;
    let doc = create_doc("file:///inline.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);

    let error = diagnostics
        .iter()
        .find(|d| d.message.contains("nonExistentOnUser"));
    assert!(error.is_some(), "Expected error in inline fragment");
}

#[test]
#[ntest::timeout(100)]
fn test_validation_unknown_fragment_spread() {
    let text = r#"
        query {
            users {
                ...UnknownFrag
            }
        }
    "#;
    let doc = create_doc("file:///spread.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);

    let error = diagnostics
        .iter()
        .find(|d| d.message.contains("Unknown fragment: UnknownFrag"));
    assert!(error.is_some(), "Expected unknown fragment error");
}

#[test]
#[ntest::timeout(100)]
fn test_validation_known_fragment_spread() {
    let text = r#"
        query {
            users {
                ...KnownFrag
            }
        }
    "#;
    let doc = create_doc("file:///known_spread.graphql", text);
    let fragments = vec![FragmentCompletionInfo {
        name: "KnownFrag".to_string(),
        type_condition: "User".to_string(),
        description: None,
        import_path: None,
        is_public: false,
        is_type_only: false,
        uri: Url::parse("file:///test.graphql").unwrap(),
        package_root: None,
        used_variables: Vec::new(),
        used_fragments: Vec::new(),
        requirements: std::collections::BTreeMap::new(),
    }];
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &fragments, None, None, false, true);

    assert!(
        diagnostics.is_empty(),
        "Expected no error for known fragment spread"
    );
}

#[test]
fn test_type_only_fragment_unused() {
    let schema_content = "type User { id: ID! } type Query { me: User }";
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        fragment UserFrag on User @type_only {
            id
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Should NOT have diagnostics for unused fragment
    let used_fragments = fnv::FnvHashSet::default();
    let diagnostics =
        doc.get_semantic_diagnostics(&schema, &[], Some(&used_fragments), None, false, true);

    assert!(
        diagnostics.is_empty(),
        "Expected no diagnostics for @type_only unused fragment, got: {:?}",
        diagnostics
    );
}

#[test]
fn test_type_only_fragment_used() {
    let schema_content = "type User { id: ID! } type Query { me: User }";
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        fragment UserFrag on User @type_only {
            id
        }
        
        query {
            me {
                ...UserFrag
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Should HAVE a warning because it's used but marked @type_only
    let mut used_fragments = fnv::FnvHashSet::default();
    used_fragments.insert("UserFrag".to_string());

    let diagnostics =
        doc.get_semantic_diagnostics(&schema, &[], Some(&used_fragments), None, false, true);

    let warning = diagnostics
        .iter()
        .find(|d| d.code == Some(NumberOrString::String("type_only_used".to_string())));
    assert!(
        warning.is_some(),
        "Expected warning for @type_only fragment being used"
    );
    assert!(warning.unwrap().message.contains("Remove @type_only"));
}

#[test]
#[ntest::timeout(100)]
fn test_validation_input_field_deprecation() {
    let schema_content = r#"
        input CreateUserInput {
          username: String!
          oldField: String @deprecated(reason: "Use newField")
          newField: String
        }
        type Query {
          test(input: CreateUserInput): String
        }
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        query Test {
            test(input: { oldField: "value" })
        }
    "#;
    let doc = create_doc("file:///input_deprecated.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    let warning = diagnostics
        .iter()
        .find(|d| d.message.contains("Input field 'oldField' is deprecated"));
    assert!(
        warning.is_some(),
        "Expected 'deprecated' warning for input field"
    );
    assert_eq!(warning.unwrap().severity, Some(DiagnosticSeverity::WARNING));
    assert!(warning.unwrap().message.contains("Use newField"));
}

#[test]
#[ntest::timeout(100)]
fn test_validation_unions_and_interfaces() {
    let schema_content = r#"
        interface Named {
          name: String!
        }
        type User implements Named {
          id: ID!
          name: String!
        }
        type Bot implements Named {
          id: ID!
          name: String!
          version: String!
        }
        union SearchResult = User | Bot
        type Query {
          search(term: String!): [SearchResult]
        }
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    // Valid query with inline fragments
    let text = r#"
        query Search($term: String!) {
            search(term: $term) {
                ... on User { id name }
                ... on Bot { id name version }
            }
        }
    "#;
    let doc = create_doc("file:///valid_union.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    assert!(
        diagnostics.is_empty(),
        "Valid union query failed: {:?}",
        diagnostics
    );

    // Invalid: field not on union
    let text = r#"
        query {
            search(term: "foo") {
                id
            }
        }
    "#;
    let doc = create_doc("file:///invalid_union.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("not found on type 'SearchResult'"))
    );

    // Valid: field on interface
    let text = r#"
        query {
            search(term: "foo") {
                ... on Named { name }
            }
        }
    "#;
    let doc = create_doc("file:///valid_interface.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);
    assert!(
        diagnostics.is_empty(),
        "Valid interface query failed: {:?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(100)]
fn test_validation_block_strings_and_comments() {
    let text = r#"
        query GetUser($id: ID! = """123""") # This is a comment
        {
            node(id: $id) {
                id
            }
        }
    "#;
    let doc = create_doc("file:///quirks.graphql", text);
    let diagnostics =
        doc.get_semantic_diagnostics(get_valid_schema(), &[], None, None, false, true);
    assert!(
        diagnostics.is_empty(),
        "Block strings or comments caused issues: {:?}",
        diagnostics
    );
}
