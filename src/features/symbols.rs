use crate::document::DocumentState;
use crate::queries::*;
use tower_lsp::lsp_types::*;
use tree_sitter::{QueryCursor, StreamingIterator};

impl DocumentState {
    pub fn get_symbols(&self) -> Vec<DocumentSymbol> {
        let query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
        });

        let mut cursor = QueryCursor::new();
        let mut symbols = Vec::new();

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let mut matches =
                cursor.matches(query, block.tree.root_node(), |node: tree_sitter::Node| {
                    let start = node.start_byte();
                    let end = node.end_byte();
                    self.rope
                        .byte_slice((start + offset)..(end + offset))
                        .chunks()
                });

            while let Some(m) = matches.next() {
                let name_node = m.captures[0].node;
                let container_node = m.captures[1].node;

                let name = self
                    .rope
                    .slice(
                        self.rope.byte_to_char(name_node.start_byte() + offset)
                            ..self.rope.byte_to_char(name_node.end_byte() + offset),
                    )
                    .to_string();

                #[allow(deprecated)]
                symbols.push(DocumentSymbol {
                    name,
                    detail: Some(format!("GraphQL {}", container_node.kind())),
                    kind: SymbolKind::STRUCT,
                    tags: None,
                    deprecated: None,
                    range: self.translate_to_file_range(container_node, offset),
                    selection_range: self.translate_to_file_range(name_node, offset),
                    children: None,
                });
            }
        }
        symbols
    }
}
