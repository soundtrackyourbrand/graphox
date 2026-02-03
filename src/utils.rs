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

pub fn get_project_files(include_patterns: &[String], exclude_patterns: &[String]) -> Vec<PathBuf> {
    use globset::{Glob, GlobSetBuilder};
    use ignore::WalkBuilder;

    let mut include_builder = GlobSetBuilder::new();
    let mut roots = Vec::new();
    let mut direct_files = Vec::new();

    for p in include_patterns {
        let p_clean = if p.starts_with("./") { &p[2..] } else { p };
        let is_glob = p_clean.contains('*')
            || p_clean.contains('?')
            || p_clean.contains('[')
            || p_clean.contains('{');
        if !is_glob {
            let path = PathBuf::from(p_clean);
            if path.is_file() {
                direct_files.push(path);
                continue;
            }
            if path.is_dir() {
                roots.push(path.clone());
                let mut p_glob = p_clean.to_string();
                if !p_glob.ends_with('/') && !p_glob.is_empty() {
                    p_glob.push('/');
                }
                p_glob.push_str("**/*");
                if let Ok(g) = Glob::new(&p_glob) {
                    include_builder.add(g);
                }
                continue;
            }
        }

        if let Ok(g) = Glob::new(p) {
            include_builder.add(g);
        }
        if p_clean != p {
            if let Ok(g) = Glob::new(p_clean) {
                include_builder.add(g);
            }
        }

        let include_path = Path::new(p_clean);
        let mut root = PathBuf::new();
        for component in include_path.components() {
            let s = component.as_os_str().to_str().unwrap_or("");
            if s.contains('*') || s.contains('?') || s.contains('[') || s.contains('{') {
                break;
            }
            root.push(component);
        }
        if root.as_os_str().is_empty() {
            roots.push(PathBuf::from("."));
        } else {
            roots.push(root);
        }
    }

    let mut exclude_builder = GlobSetBuilder::new();
    for p in exclude_patterns {
        let p_clean = if p.starts_with("./") { &p[2..] } else { p };
        if let Ok(g) = Glob::new(p_clean) {
            exclude_builder.add(g);
        }
        if p != p_clean {
            if let Ok(g) = Glob::new(p) {
                exclude_builder.add(g);
            }
        }
    }

    let include_set = include_builder
        .build()
        .unwrap_or_else(|_| GlobSetBuilder::new().build().unwrap());
    let exclude_set = exclude_builder
        .build()
        .unwrap_or_else(|_| GlobSetBuilder::new().build().unwrap());

    let mut files = direct_files;

    if roots.is_empty() && include_patterns.iter().any(|p| p.contains('*')) {
        roots.push(PathBuf::from("."));
    }

    if !roots.is_empty() {
        roots.sort();
        let mut unique_roots = Vec::new();
        for root in roots {
            if !unique_roots.iter().any(|r| root.starts_with(r)) {
                unique_roots.push(root);
            }
        }

        let mut walk_builder = WalkBuilder::new(&unique_roots[0]);
        for root in &unique_roots[1..] {
            walk_builder.add(root);
        }

        let walk = walk_builder
            .add_custom_ignore_filename(".graphqlignore")
            .hidden(false)
            .build();

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        for entry in walk.filter_map(|e| e.ok()) {
            if entry.file_type().is_some_and(|ft| ft.is_file()) {
                let path = entry.path();
                if is_relevant_file(path) {
                    let mut matched = include_set.is_match(path);

                    if !matched {
                        if let Some(rel_to_cwd) = pathdiff::diff_paths(path, &cwd) {
                            if include_set.is_match(&rel_to_cwd) {
                                matched = true;
                            }
                        }
                    }

                    if !matched {
                        if let Some(file_name) = path.file_name() {
                            if include_set.is_match(file_name) {
                                matched = true;
                            }
                        }
                    }

                    if matched {
                        let mut excluded = exclude_set.is_match(path);
                        if !excluded {
                            if let Some(rel_to_cwd) = pathdiff::diff_paths(path, &cwd) {
                                if exclude_set.is_match(&rel_to_cwd) {
                                    excluded = true;
                                }
                            }
                        }
                        if !excluded {
                            files.push(path.to_owned());
                        }
                    }
                }
            }
        }
    }

    files.sort();
    files.dedup();
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
