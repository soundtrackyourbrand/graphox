use crate::document::DocumentState;
use crate::queries::*;
use crate::utils::token_type_to_legend_index;
use tower_lsp::lsp_types::*;
use tree_sitter::{QueryCursor, StreamingIterator};

impl DocumentState {
    pub fn get_semantic_tokens(&self) -> Vec<SemanticToken> {
        let query = GQL_SEMANTIC_TOKEN_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, SEMANTIC_TOKEN_QUERY).unwrap()
        });

        let mut cursor = QueryCursor::new();

        let mut tokens = Vec::new();

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
                for cap in m.captures {
                    let token_type_name = &query.capture_names()[cap.index as usize];
                    let token_type = token_type_to_legend_index(token_type_name);

                    let range = self.translate_to_file_range(cap.node, offset);

                    tokens.push(SemanticToken {
                        delta_line: range.start.line,
                        delta_start: range.start.character,
                        length: range.end.character - range.start.character,
                        token_type,
                        token_modifiers_bitset: 0,
                    });
                }
            }
        }

        tokens
    }
}
