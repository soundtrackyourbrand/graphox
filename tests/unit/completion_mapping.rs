use crate::document::{DocumentLanguage, DocumentState};
use crate::support::create_doc;
use tower_lsp::lsp_types::Position;
use tree_sitter::Parser;

#[test]
fn mapping_graphql_in_tsx_template_literal() {
    let src = "const q = graphql(/* GraphQL */ `\nquery {\n  users {\n    ... on \n  }\n}\n`);\n";
    let uri = Url::parse("file:///tmp/test.tsx").unwrap();
    let mut parser = Parser::new();
    parser
        .set_language(tree_sitter_typescript::LANGUAGE_TSX.into())
        .unwrap();
    let doc = create_doc(uri.as_str(), src);

    // Expect exactly one graphql block and its offsets to map inside the file
    let blocks = doc.get_graphql_trees();
    assert_eq!(blocks.len(), 1);
    let block = &blocks[0];
    let root = block.tree.root_node();
    let start = block.offset;
    let end = start + root.end_byte();
    assert!(start < end);

    // Cursor at the inline fragment position -> should map into the block
    let pos = Position::new(4, 11);
    let byte = doc.position_to_byte(pos);
    assert!(
        byte >= start && byte <= end,
        "byte {} not in block {}..{}",
        byte,
        start,
        end
    );
}
