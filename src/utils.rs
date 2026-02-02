use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::SemanticTokenType;

pub const SEMANTIC_TOKEN_LEGEND: &[SemanticTokenType] = &[
    SemanticTokenType::VARIABLE,
    SemanticTokenType::TYPE,
    SemanticTokenType::STRING,
];

#[repr(u32)]
pub enum SemanticTokenKind {
    Variable = 0,
    Type = 1,
    String = 2,
}

pub fn is_relevant_file(path: &Path) -> bool {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    matches!(
        ext,
        "graphql" | "gql" | "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs"
    )
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
            if entry.file_type().is_some_and(|ft| ft.is_file()) {
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

pub fn find_package_root(start_path: &Path) -> Option<PathBuf> {
    let mut current = if start_path.is_dir() {
        start_path.to_path_buf()
    } else {
        start_path.parent()?.to_path_buf()
    };

    loop {
        if current.join("package.json").exists() {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

/// Simple interpolation masker for template strings.
/// Replaces ${...} with spaces of the same length to preserve offsets.
pub fn mask_interpolations(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            result.push_str("  ");
            chars.next(); // consume '{'
            let mut depth = 1;
            while depth > 0 {
                if let Some(inner_c) = chars.next() {
                    match inner_c {
                        '{' => {
                            depth += 1;
                            result.push(' ');
                        }
                        '}' => {
                            depth -= 1;
                            result.push(' ');
                        }
                        '\n' => result.push('\n'),
                        _ => result.push(' '),
                    }
                } else {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}
