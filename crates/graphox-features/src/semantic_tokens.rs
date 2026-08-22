use graphox_core::document::DocumentState;
use graphox_core::utils::SemanticTokenKind;
use ls_types::*;
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

        // Keyword: operation_type (query, mutation, subscription)
        if kind == "operation_type" {
            tokens.push(RawToken {
                range: self.translate_to_file_range(node, offset),
                token_type: SemanticTokenKind::Keyword as u32,
            });
            captured = true;
        }
        // Enum value: enum_value
        else if kind == "enum_value" {
            tokens.push(RawToken {
                range: self.translate_to_file_range(node, offset),
                token_type: SemanticTokenKind::Enum as u32,
            });
            captured = true;
        }
        // String value
        else if kind == "string_value" {
            tokens.push(RawToken {
                range: self.translate_to_file_range(node, offset),
                token_type: SemanticTokenKind::String as u32,
            });
            captured = true;
        }
        // Named type
        else if kind == "named_type" {
            tokens.push(RawToken {
                range: self.translate_to_file_range(node, offset),
                token_type: SemanticTokenKind::Type as u32,
            });
            captured = true;
        }
        // Field name in selections (e.g., user, soundZoneUpdate)
        else if kind == "field" {
            // Get the name child of the field
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "name" {
                    tokens.push(RawToken {
                        range: self.translate_to_file_range(child, offset),
                        token_type: SemanticTokenKind::Property as u32,
                    });
                    break;
                }
            }
            captured = true;
        }
        // Variable: variable node (the $name part)
        else if kind == "variable" {
            tokens.push(RawToken {
                range: self.translate_to_file_range(node, offset),
                token_type: SemanticTokenKind::Variable as u32,
            });
            captured = true;
        }
        // Name in different contexts - determine based on parent
        else if kind == "name" {
            let parent = node.parent();
            let grandparent = parent.and_then(|p| p.parent());

            // Skip if parent is named_type (we handle that above)
            if parent.map(|p| p.kind() == "named_type").unwrap_or(false) {
                // Don't capture - named_type handler does this
            }
            // Operation definition name (e.g., SonarZone in `subscription SonarZone`)
            // or Fragment definition name (e.g., UserFields in `fragment UserFields on User`)
            else if parent
                .map(|p| p.kind() == "operation_definition" || p.kind() == "fragment_name")
                .unwrap_or(false)
            {
                tokens.push(RawToken {
                    range: self.translate_to_file_range(node, offset),
                    token_type: SemanticTokenKind::Function as u32,
                });
                captured = true;
            }
            // Fragment spread name (e.g., ...UserFields)
            else if parent
                .map(|p| p.kind() == "fragment_spread")
                .unwrap_or(false)
            {
                // The fragment_spread has a fragment_name child which has a name child
                // This is already handled by the name->fragment_name->fragment_spread path above
            }
            // Argument name in field arguments (e.g., id: $id)
            // or Directive name (e.g., @skip, @include)
            else if parent
                .map(|p| p.kind() == "argument" || p.kind() == "directive")
                .unwrap_or(false)
            {
                tokens.push(RawToken {
                    range: self.translate_to_file_range(node, offset),
                    token_type: SemanticTokenKind::Property as u32,
                });
                captured = true;
            }
            // Variable definition (in ($id: ID!))
            else if parent
                .map(|p| p.kind() == "variable_definition")
                .unwrap_or(false)
                || grandparent
                    .map(|p| p.kind() == "variable_definition")
                    .unwrap_or(false)
            {
                // Already handled by variable node above, skip
            }
            // Default to Variable for other name contexts
            else {
                tokens.push(RawToken {
                    range: self.translate_to_file_range(node, offset),
                    token_type: SemanticTokenKind::Variable as u32,
                });
                captured = true;
            }
        }

        if !captured || kind == "named_type" || kind == "field" {
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
