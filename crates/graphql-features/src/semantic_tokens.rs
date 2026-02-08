use graphql_core::document::DocumentState;
use graphql_core::utils::SemanticTokenKind;
use lsp_types::*;
use tree_sitter::Node;

pub trait DocumentSemanticTokens {
    fn get_semantic_tokens(&self) -> Vec<SemanticToken>;
    fn collect_tokens_manual(&self, node: Node, offset: usize, tokens: &mut Vec<RawToken>);
}

impl DocumentSemanticTokens for DocumentState {
    fn get_semantic_tokens(&self) -> Vec<SemanticToken> {
        let mut raw_tokens = Vec::new();

        for block in self.get_graphql_trees() {
            self.collect_tokens_manual(block.tree.root_node(), block.offset, &mut raw_tokens);
        }

        // 1. Sort tokens by line and then by character
        raw_tokens.sort_by(|a, b| {
            if a.range.start.line != b.range.start.line {
                a.range.start.line.cmp(&b.range.start.line)
            } else {
                a.range.start.character.cmp(&b.range.start.character)
            }
        });

        // 2. Delta-encode for LSP
        let mut last_line = 0;
        let mut last_start = 0;
        let mut encoded_tokens = Vec::new();

        for token in raw_tokens {
            let line = token.range.start.line;
            let start = token.range.start.character;

            let delta_line = line - last_line;
            let delta_start = if delta_line == 0 {
                start - last_start
            } else {
                start
            };

            encoded_tokens.push(SemanticToken {
                delta_line,
                delta_start,
                length: token.range.end.character - token.range.start.character,
                token_type: token.token_type,
                token_modifiers_bitset: 0,
            });

            last_line = line;
            last_start = start;
        }

        encoded_tokens
    }

    fn collect_tokens_manual(&self, node: Node, offset: usize, tokens: &mut Vec<RawToken>) {
        let kind = node.kind();
        let mut captured = false;

        if kind == "named_type" {
            tokens.push(RawToken {
                range: self.translate_to_file_range(node, offset),
                token_type: SemanticTokenKind::Type as u32,
            });
            captured = true;
        } else if kind == "string_value" {
            tokens.push(RawToken {
                range: self.translate_to_file_range(node, offset),
                token_type: SemanticTokenKind::String as u32,
            });
            captured = true;
        } else if kind == "name"
            && let Some(parent) = node.parent()
            && parent.kind() != "named_type"
        {
            tokens.push(RawToken {
                range: self.translate_to_file_range(node, offset),
                token_type: SemanticTokenKind::Variable as u32,
            });
            captured = true;
        }

        if !captured || kind == "named_type" {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.collect_tokens_manual(child, offset, tokens);
            }
        }
    }
}

pub struct RawToken {
    pub range: Range,
    pub token_type: u32,
}
