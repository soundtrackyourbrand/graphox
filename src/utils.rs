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

pub fn is_relevant_file(path: &Path) -> bool {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    match ext {
        "graphql" | "gql" | "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs" => true,
        _ => false,
    }
}

pub fn get_project_files(include_glob: &str) -> Vec<PathBuf> {
    use glob::glob;
    use ignore::WalkBuilder;

    let include_path = Path::new(include_glob);
    let (root, pattern) = if include_glob.contains('*') {
        let mut fixed_part = PathBuf::new();
        let mut pattern_part = String::new();
        let mut found_wildcard = false;
        for component in include_path.components() {
            let s = component.as_os_str().to_str().unwrap_or("");
            if !found_wildcard && !s.contains('*') {
                fixed_part.push(s);
            } else {
                found_wildcard = true;
                if !pattern_part.is_empty() {
                    pattern_part.push(std::path::MAIN_SEPARATOR);
                }
                pattern_part.push_str(s);
            }
        }
        if fixed_part.as_os_str().is_empty() {
            (PathBuf::from("."), include_glob.to_string())
        } else {
            (fixed_part, pattern_part)
        }
    } else {
        (include_path.to_path_buf(), String::new())
    };

    let mut files = Vec::new();
    if root.exists() {
        let walk = WalkBuilder::new(&root)
            .add_custom_ignore_filename(".graphqlignore")
            .build();

        for entry in walk.filter_map(|e| e.ok()) {
            if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                let path = entry.path().to_owned();
                if pattern.is_empty() {
                    files.push(path);
                } else {
                    let matches = glob(include_glob)
                        .map(|entries| {
                            entries.filter_map(|e| e.ok()).any(|p| {
                                std::fs::canonicalize(&p).ok() == std::fs::canonicalize(&path).ok()
                            })
                        })
                        .unwrap_or(false);
                    if matches {
                        files.push(path);
                    }
                }
            }
        }
    }
    files
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
