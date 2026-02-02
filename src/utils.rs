use tower_lsp::lsp_types::SemanticTokenType;

pub fn mask_interpolations(text: &str) -> String {
    let mut masked = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            // We found a ${ ... }
            masked.push(' '); // Replace '$'
            masked.push(' '); // Replace '{'
            chars.next(); // Consume '{'

            let mut depth = 1;
            while depth > 0 {
                if let Some(inner_c) = chars.next() {
                    if inner_c == '{' {
                        depth += 1;
                    }
                    if inner_c == '}' {
                        depth -= 1;
                    }
                    masked.push(' '); // Mask everything inside with whitespace
                } else {
                    break;
                }
            }
        } else {
            masked.push(c);
        }
    }
    masked
}

pub fn token_type_to_legend_index(token_type: &str) -> u32 {
    match token_type {
        "variable" => 0,
        "type" => 1,
        "keyword" => 2,
        "enum" => 3,
        "string" => 4,
        _ => 0,
    }
}

pub const SEMANTIC_TOKEN_LEGEND: &[SemanticTokenType] = &[
    SemanticTokenType::VARIABLE,
    SemanticTokenType::TYPE,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::ENUM,
    SemanticTokenType::STRING,
];
