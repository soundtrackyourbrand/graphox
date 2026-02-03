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
    use globset::{Glob, GlobSetBuilder};
    use ignore::WalkBuilder;

    let is_glob = include_glob.contains('*')
        || include_glob.contains('?')
        || include_glob.contains('[')
        || include_glob.contains('{');

    // If it's not a glob, it might be a file or a directory
    if !is_glob {
        let p = PathBuf::from(include_glob);
        if p.is_file() {
            return vec![p];
        }
        if p.is_dir() {
            let mut files = Vec::new();
            let walk = WalkBuilder::new(&p)
                .add_custom_ignore_filename(".graphqlignore")
                .hidden(false)
                .build();
            for entry in walk.filter_map(|e| e.ok()) {
                if entry.file_type().is_some_and(|ft| ft.is_file()) {
                    let path = entry.path();
                    if is_relevant_file(path) {
                        files.push(path.to_owned());
                    }
                }
            }
            return files;
        }
    }

    let glob = match Glob::new(include_glob) {
        Ok(g) => g,
        Err(_) => return Vec::new(),
    };

    let mut builder = GlobSetBuilder::new();
    builder.add(glob);
    let glob_set = builder.build().unwrap();

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    // Determine a search root to avoid walking the whole drive if possible
    let include_path = Path::new(include_glob);
    let mut root = PathBuf::new();
    for component in include_path.components() {
        let s = component.as_os_str().to_str().unwrap_or("");
        if s.contains('*') || s.contains('?') || s.contains('[') || s.contains('{') {
            break;
        }
        root.push(component);
    }

    if root.as_os_str().is_empty() {
        root = PathBuf::from(".");
    } else if !root.is_dir() {
        if let Some(parent) = root.parent() {
            root = parent.to_path_buf();
        } else {
            root = PathBuf::from(".");
        }
    }

    let mut files = Vec::new();
    let walk = WalkBuilder::new(&root)
        .add_custom_ignore_filename(".graphqlignore")
        .hidden(false)
        .build();

    for entry in walk.filter_map(|e| e.ok()) {
        if entry.file_type().is_some_and(|ft| ft.is_file()) {
            let path = entry.path();

            // Try matching as provided
            if glob_set.is_match(path) {
                files.push(path.to_owned());
                continue;
            }

            // Try matching relative to CWD
            if let Some(rel_to_cwd) = pathdiff::diff_paths(path, &cwd) {
                if glob_set.is_match(&rel_to_cwd) {
                    files.push(path.to_owned());
                    continue;
                }
            }

            // Try matching just the file name if the glob is just a pattern
            if let Some(file_name) = path.file_name() {
                if glob_set.is_match(file_name) {
                    files.push(path.to_owned());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_interpolations() {
        let input = "query { user(id: ${userId}) { name } }";
        let masked = mask_interpolations(input);
        assert_eq!(masked.len(), input.len());
        assert!(masked.contains("user(id: "));
        assert!(masked.contains(") { name }"));

        let nested = "query { user(id: ${getId({a: 1})}) { name } }";
        let masked_nested = mask_interpolations(nested);
        assert_eq!(masked_nested.len(), nested.len());

        let multi_line = "query {\n  ${fragment}\n  user { id }\n}";
        let masked_multi_line = mask_interpolations(multi_line);
        assert_eq!(masked_multi_line.len(), multi_line.len());
        assert_eq!(
            masked_multi_line.lines().count(),
            multi_line.lines().count()
        );
    }
}
