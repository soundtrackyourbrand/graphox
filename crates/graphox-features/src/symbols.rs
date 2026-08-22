use graphox_core::document::DocumentState;
use graphox_core::queries::*;
use ls_types::*;
use tree_sitter::{QueryCursor, StreamingIterator};

pub trait DocumentSymbols {
    fn get_symbols(&self) -> Vec<DocumentSymbol>;
}

impl DocumentSymbols for DocumentState {
    fn get_symbols(&self) -> Vec<DocumentSymbol> {
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
                let mut name_node = None;
                let mut container_node = None;

                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    if cap_name == "symbol.name" {
                        name_node = Some(cap.node);
                    } else if cap_name == "symbol.container" {
                        container_node = Some(cap.node);
                    }
                }

                if let (Some(name_node), Some(container_node)) = (name_node, container_node) {
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
        }
        symbols
    }
}
