use graphql_rust::DocumentState;
use tower_lsp::lsp_types::Url;

#[test]
fn test_template_extraction_variants() {
    let text = r#"
        // Standard tag
        const q1 = gql`query { foo }`;
        
        // Comment tag
        const q2 = /* GraphQL */ `query { bar }`;
        
        // Untagged (should be ignored)
        const q3 = `query { ignore_me }`;
        
        // Lowercase comment tag
        const q4 = /* graphql */ `query { baz }`;
        
        // Comment tag with spaces
        const q5 = /*   GraphQL   */ `query { qux }`;
    "#;

    let uri = Url::parse("file:///test.ts").unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .unwrap();

    let doc = DocumentState::new(uri, text, parser);
    let blocks = doc.get_graphql_trees();

    assert_eq!(
        blocks.len(),
        4,
        "Should find 4 GraphQL blocks (q1, q2, q4, q5)"
    );

    let contents: Vec<String> = blocks
        .iter()
        .map(|b| {
            let root = b.tree.root_node();
            doc.get_node_text(root, b.offset)
        })
        .collect();

    assert!(contents.iter().any(|c| c.contains("foo")), "Missing q1");
    assert!(contents.iter().any(|c| c.contains("bar")), "Missing q2");
    assert!(contents.iter().any(|c| c.contains("baz")), "Missing q4");
    assert!(contents.iter().any(|c| c.contains("qux")), "Missing q5");
    assert!(
        !contents.iter().any(|c| c.contains("ignore_me")),
        "Should not have extracted q3"
    );
}

#[test]
fn test_template_deduplication() {
    // This tests that we don't extract the same template twice if both query patterns match
    // although with current implementation it's unlikely, it's good to be sure.
    let text = r#"
        const q1 = gql`query { foo }`;
    "#;

    let uri = Url::parse("file:///test.ts").unwrap();
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .unwrap();

    let doc = DocumentState::new(uri, text, parser);
    let blocks = doc.get_graphql_trees();

    assert_eq!(
        blocks.len(),
        1,
        "Should only find 1 GraphQL block for tagged template"
    );
}
