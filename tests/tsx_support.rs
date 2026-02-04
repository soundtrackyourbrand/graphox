use graphql_rust::{DocumentLanguage, DocumentState};
use tower_lsp::lsp_types::Url;

#[test]
#[ntest::timeout(100)]
fn test_user_repro_pattern() {
    let text = r#"
        export function BlockedTracksTable(props: BlockedTracksTableProps) {
          return (
            <>
              <TrackList
                blocked={<Blocked isBlocked={isBlocked} />}
              />
            </>
          )
        }

        const BlockedSongInfoFragmentDoc = graphql(/* GraphQL */ `
          fragment BlockedSongInfo on BlockedTrack {
            id
            reasons
          }
        `)
    "#;

    let uri = Url::parse("file:///test.tsx").unwrap();
    let language = DocumentLanguage::from_uri(&uri);
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language.get_parser_language())
        .unwrap();

    let doc = DocumentState::new(uri, text, parser);
    let blocks = doc.get_graphql_trees();

    assert_eq!(
        blocks.len(),
        1,
        "Should have found the GraphQL block in TSX"
    );

    let fragments = doc.fragments();
    assert_eq!(fragments.len(), 1, "Should have found the fragment in TSX");
    assert_eq!(fragments[0].name, "BlockedSongInfo");
}

#[test]
#[ntest::timeout(100)]
fn test_print_ts_tree() {
    let text = r#"
        const q1 = gql`query { foo }`;
        const q2 = graphql`query { bar }`;
        const q3 = graphql(/* GraphQL */ `query { baz }`);
    "#;

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into())
        .unwrap();
    let tree = parser.parse(text, None).unwrap();
    println!("TS TREE: {}", tree.root_node().to_string());
}

#[test]
#[ntest::timeout(100)]
fn test_print_gql_completion_trees() {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_graphql::LANGUAGE.into())
        .unwrap();

    let text1 = "query GetUser($userId: ID!) { user(id: $) }";
    let tree1 = parser.parse(text1, None).unwrap();
    println!("VAR TREE: {}", tree1.root_node().to_string());

    let text2 = "fragment MyFrag on  { id }";
    let tree2 = parser.parse(text2, None).unwrap();
    println!("TYPE COND TREE: {}", tree2.root_node().to_string());
}

#[test]
#[ntest::timeout(100)]
fn test_graphql_tag_repro() {
    let text = r#"
        const q = graphql`query { foo }`;
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
        "Should support 'graphql' tag as well as 'gql'"
    );
}
