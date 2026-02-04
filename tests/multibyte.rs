use apollo_compiler::Schema;
use graphql_rust::DocumentState;
use tower_lsp::lsp_types::*;

fn create_ts_doc(text: &str) -> DocumentState {
    let uri = Url::parse("file:///test.ts").unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .unwrap();
    DocumentState::new(uri, text, parser)
}

#[test]
fn test_multibyte_range_calculation() {
    let schema_content = r#"
        type Query {
          oldField: String @deprecated(reason: "Use newField")
          newField: String
        }
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql").unwrap();

    // 🚀 is 4 bytes in UTF-8, 1 char in Unicode, 2 code units in UTF-16
    let text = r#"
// 🚀 emoji shift
const q = gql`
  query {
    oldField
  }
`;
"#;
    let doc = create_ts_doc(text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, None);

    assert_eq!(diagnostics.len(), 1);
    let d = &diagnostics[0];
    assert!(d.message.contains("oldField"));

    // Line 4 (0-indexed), "    oldField"
    // "    " is 4 spaces
    // The line is: "    oldField"
    // So character should be 4.
    assert_eq!(d.range.start.line, 4);
    assert_eq!(d.range.start.character, 4);
    assert_eq!(d.range.end.character, 12);
}

#[test]
fn test_emoji_in_query_comment() {
    let schema_content = r#"
        type Query {
          field: String
        }
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql").unwrap();

    let text = r#"
const q = gql`
  query {
    # 🚀 comment with emoji
    field
    unknownField
  }
`;
"#;
    let doc = create_ts_doc(text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, None);

    let error = diagnostics
        .iter()
        .find(|d| d.message.contains("unknownField") && d.range.start.line == 5)
        .expect("Should find precise error for unknownField");

    // Line 5: "    unknownField"
    // Character should be 4.
    assert_eq!(error.range.start.line, 5);
    assert_eq!(error.range.start.character, 4);
}

#[test]
fn test_goto_definition_with_emoji() {
    let text = r#"
fragment UserFrag on User { id }
const q = gql`
  query {
    # 🚀 emoji shift
    ...UserFrag
  }
`;
"#;
    let doc = create_ts_doc(text);

    // Line 5: "    ...UserFrag"
    // "...UserFrag" starts at column 4
    // "UserFrag" starts at column 7
    let pos = Position::new(5, 7);
    let symbol = doc.get_symbol_at_position(pos);
    assert_eq!(symbol, Some("UserFrag".to_string()));
}
