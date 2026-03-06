use crate::support::create_doc;

#[test]
#[ntest::timeout(300)]
fn test_tag_variations() {
    let text = r#"
        const q1 = gql`query { foo }`;
        const q2 = graphql`query { bar }`;
        const q3 = /* GraphQL */ `query { baz }`;
        const q4 = /*graphql*/ `query { qux }`;
        const q5 = /* GraphQL */`query { quux }`;
    "#;

    let doc = create_doc("file:///test.ts", text);
    let blocks = doc.get_graphql_trees();

    assert_eq!(
        blocks.len(),
        5,
        "Should find 5 GraphQL blocks (gql, graphql, /* GraphQL */, /*graphql*/, /* GraphQL */`)"
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
    assert!(contents.iter().any(|c| c.contains("baz")), "Missing q3");
    assert!(contents.iter().any(|c| c.contains("qux")), "Missing q4");
    assert!(contents.iter().any(|c| c.contains("quux")), "Missing q5");
}

#[test]
#[ntest::timeout(300)]
fn test_tag_in_arrow_function() {
    let text = r#"
        const getQuery = () => gql`query { foo }`;
        const getQuery2 = () => {
            return graphql`query { bar }`;
        };
        const arrowFn = (x: number) => /* GraphQL */ `query { baz }`;
    "#;

    let doc = create_doc("file:///test.ts", text);
    let blocks = doc.get_graphql_trees();

    assert_eq!(
        blocks.len(),
        3,
        "Should find 3 GraphQL blocks in arrow functions"
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
    assert!(contents.iter().any(|c| c.contains("baz")), "Missing q3");
}

#[test]
#[ntest::timeout(300)]
fn test_tag_multiple_on_same_line() {
    let text = r#"
        const a = gql`query A { foo }`; const b = gql`query B { bar }`;
        const c = /* GraphQL */ `query C { baz }`; const d = graphql`query D { qux }`;
    "#;

    let doc = create_doc("file:///test.ts", text);
    let blocks = doc.get_graphql_trees();

    assert_eq!(
        blocks.len(),
        4,
        "Should find 4 GraphQL blocks on same/different lines"
    );

    let contents: Vec<String> = blocks
        .iter()
        .map(|b| {
            let root = b.tree.root_node();
            doc.get_node_text(root, b.offset)
        })
        .collect();

    assert!(
        contents.iter().any(|c| c.contains("foo")),
        "Missing query A"
    );
    assert!(
        contents.iter().any(|c| c.contains("bar")),
        "Missing query B"
    );
    assert!(
        contents.iter().any(|c| c.contains("baz")),
        "Missing query C"
    );
    assert!(
        contents.iter().any(|c| c.contains("qux")),
        "Missing query D"
    );
}

#[test]
#[ntest::timeout(300)]
fn test_tag_whitespace_variations() {
    let text = r#"
        const q1 = gql   `query { foo }`;
        const q2 = graphql   `query { bar }`;
        const q3 = /* GraphQL */   `query { baz }`;
        const q4 = gql
`query { qux }`;
        const q5 = gql	`query { quux }`;
    "#;

    let doc = create_doc("file:///test.ts", text);
    let blocks = doc.get_graphql_trees();

    assert_eq!(
        blocks.len(),
        5,
        "Should find 5 GraphQL blocks with varying whitespace"
    );
}

#[test]
#[ntest::timeout(300)]
fn test_tag_template_deduplication() {
    let text = r#"
        const q1 = gql`query { foo }`;
        const q2 = gql`query { foo }`;
        const q3 = graphql`query { foo }`;
    "#;

    let doc = create_doc("file:///test.ts", text);
    let blocks = doc.get_graphql_trees();

    assert_eq!(
        blocks.len(),
        3,
        "Should find 3 distinct GraphQL blocks (deduplication is per location, not per content)"
    );
}

#[test]
#[ntest::timeout(300)]
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

    let doc = create_doc("file:///test.ts", text);
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
#[ntest::timeout(300)]
fn test_template_deduplication() {
    // This tests that we don't extract the same template twice if both query patterns match
    // although with current implementation it's unlikely, it's good to be sure.
    let text = r#"
        const q1 = gql`query { foo }`;
    "#;

    let doc = create_doc("file:///test.ts", text);
    let blocks = doc.get_graphql_trees();

    assert_eq!(
        blocks.len(),
        1,
        "Should only find 1 GraphQL block for tagged template"
    );
}
