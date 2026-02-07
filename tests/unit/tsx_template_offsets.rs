use graphql_rust::{DocumentLanguage, DocumentState};
use tower_lsp::lsp_types::Url;

#[test]
#[ntest::timeout(100)]
fn test_various_tsx_template_shapes_map_offsets() {
    let fixtures = vec![
        ("tagged_gql", "const q = gql`fragment F on User { id }`;"),
        (
            "tagged_graphql",
            "const q = graphql`fragment F on User { id }`;",
        ),
        (
            "call_with_comment",
            "const q = graphql(/* GraphQL */ `fragment F on User { id }`);",
        ),
        (
            "call_no_comment",
            "const q = graphql(`fragment F on User { id }`);",
        ),
        (
            "interpolations",
            "const q = graphql(`fragment F on User { id ${foo} }`);",
        ),
    ];

    for (name, src) in fixtures {
        let uri = Url::parse(&format!("file:///{}.tsx", name)).unwrap();
        let language = DocumentLanguage::from_uri(&uri);
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&language.get_parser_language())
            .unwrap();
        let doc = DocumentState::new(uri, src, parser);
        let blocks = doc.get_graphql_trees();
        assert_eq!(blocks.len(), 1, "{}: should have one graphql block", name);
        let block = &blocks[0];

        // Find substring "fragment F" inside the source and map that position
        // to a byte offset. Using line 0 with a UTF-16 char position equal to
        // the substring index works for these single-line fixtures.
        let frag_pos = src
            .find("fragment F")
            .or_else(|| src.find("fragment F on"))
            .unwrap_or(0);

        let position = tower_lsp::lsp_types::Position::new(0, frag_pos as u32);
        let byte = doc.position_to_byte(position);
        let abs_range = doc.get_absolute_byte_range(block.tree.root_node(), block.offset);
        assert!(
            byte >= abs_range.start && byte <= abs_range.end,
            "{}: mapped byte {} not inside block {:?} (block.offset={})",
            name,
            byte,
            abs_range,
            block.offset
        );
    }
}
