use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::SemanticTokenType;

pub fn find_package_root(path: &Path) -> Option<PathBuf> {
    let mut current = path.parent();

    while let Some(dir) = current {
        if dir.join("package.json").exists() {
            return Some(dir.to_path_buf());
        }
        current = dir.parent();
    }

    None
}

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

#[derive(Debug, Clone, Copy)]
#[repr(u32)]
pub enum SemanticTokenKind {
    Variable = 0,
    Type = 1,
    Keyword = 2,
    Enum = 3,
    String = 4,
}

pub const SEMANTIC_TOKEN_LEGEND: &[SemanticTokenType] = &[
    SemanticTokenType::VARIABLE,
    SemanticTokenType::TYPE,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::ENUM,
    SemanticTokenType::STRING,
];
