use crate::support::create_doc;
use graphox::features::diagnostics::DocumentDiagnostics;
// No direct Url or DocumentLanguage use in this test; rely on support::create_doc

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

    let doc = create_doc("file:///test.tsx", text);
    let blocks = doc.get_graphql_trees();

    assert_eq!(
        blocks.len(),
        1,
        "Should have found the GraphQL block in TSX"
    );

    let fragments = doc.fragments();
    assert_eq!(fragments.len(), 1, "Should have found the fragment in TSX");
    assert_eq!(fragments[0].name.as_ref(), "BlockedSongInfo");
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
    println!("TS TREE: {}", tree.root_node());
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
    println!("VAR TREE: {}", tree1.root_node());

    let text2 = "fragment MyFrag on  { id }";
    let tree2 = parser.parse(text2, None).unwrap();
    println!("TYPE COND TREE: {}", tree2.root_node());
}

#[test]
#[ntest::timeout(100)]
fn test_graphql_tag_repro() {
    let text = r#"
        const q = graphql`query { foo }`;
    "#;

    let doc = create_doc("file:///test.ts", text);
    let blocks = doc.get_graphql_trees();

    assert_eq!(
        blocks.len(),
        1,
        "Should support 'graphql' tag as well as 'gql'"
    );
}

#[test]
#[ntest::timeout(100)]
fn test_multiple_graphql_blocks_fragment_spreads() {
    let text = r#"
        const query = graphql(`
          query PlaylistPage($id: ID!) {
            playlist(id: $id) {
              ...PlaylistPage
            }
          }
        `);

        const fragment = graphql(`
          fragment PlaylistPage on Playlist {
            id
            permissions
          }
        `);

        const subscription = graphql(`
          subscription PlaylistSubscription($id: ID!) {
            playlistUpdate(input: { playlist: $id }) {
              playlist {
                ...PlaylistPage
              }
            }
          }
        `);
    "#;

    let doc = create_doc("file:///test.ts", text);
    let blocks = doc.get_graphql_trees();

    assert_eq!(blocks.len(), 3, "Should have found 3 GraphQL blocks");

    let fragments = doc.fragments();
    assert_eq!(
        fragments.len(),
        1,
        "Should have found 1 fragment definition"
    );
    assert_eq!(fragments[0].name.as_ref(), "PlaylistPage");

    let spreads = doc.fragment_spreads;
    assert_eq!(
        spreads.len(),
        2,
        "Should have found 2 fragment spreads across different blocks"
    );
    assert!(spreads.iter().all(|s| s.as_ref() == "PlaylistPage"));
}

#[test]
#[ntest::timeout(100)]
fn test_multiple_graphql_blocks_variables_fragment_interaction() {
    let text = r#"
        const fragment = graphql(`
          fragment PlaylistPage on Playlist {
            id
            permissions @include(if: $showPermissions)
          }
        `);

        const query = graphql(`
          query PlaylistPage($id: ID!, $showPermissions: Boolean!) {
            playlist(id: $id) {
              ...PlaylistPage
            }
          }
        `);
    "#;

    let doc = create_doc("file:///test.ts", text);

    // 1. Check fragment extraction (metadata)
    let fragments = doc.fragments();
    assert_eq!(fragments.len(), 1);
    assert_eq!(fragments[0].name.as_ref(), "PlaylistPage");
    assert!(
        fragments[0]
            .used_variables
            .iter()
            .any(|s| s.as_ref() == "showPermissions")
    );

    // 2. Check full diagnostic flow (LSP context)
    let schema_content = "type Playlist { id: ID! permissions: [String] } type Query { playlist(id: ID!): Playlist }";
    let schema = apollo_compiler::Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, None, false, true);

    // Should NOT have undefined variable error for $showPermissions
    let undefined_vars: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("Undefined variable"))
        .collect();
    assert!(
        undefined_vars.is_empty(),
        "Should not have undefined variable errors: {:?}",
        undefined_vars
    );
}
