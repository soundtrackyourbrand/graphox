use colored::*;
use std::path::{Path, PathBuf};
use tower_lsp::lsp_types::SemanticTokenType;
use tree_sitter::StreamingIterator;

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
    if path
        .components()
        .any(|c| c.as_os_str() == "node_modules" || c.as_os_str() == ".git")
    {
        return false;
    }

    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let is_ext_relevant = matches!(
        ext,
        "graphql" | "gql" | "ts" | "tsx" | "mts" | "cts" | "js" | "jsx" | "mjs" | "cjs"
    );

    if !is_ext_relevant {
        return false;
    }

    // Exclude generated files
    if let Some(file_name) = path.file_name().and_then(|s| s.to_str())
        && (file_name.ends_with(".codegen.ts")
            || file_name == "manifest.json"
            || file_name == "permissions.ts")
        {
            return false;
        }

    true
}

pub fn get_project_files(
    include_patterns: &[String],
    exclude_patterns: &[String],
    base_dir: &Path,
) -> Vec<PathBuf> {
    use globset::{Glob, GlobSetBuilder};
    use ignore::WalkBuilder;

    let mut include_builder = GlobSetBuilder::new();
    let mut roots = Vec::new();
    let mut direct_files = Vec::new();

    for p in include_patterns {
        let p_clean = if let Some(stripped) = p.strip_prefix("./") {
            stripped
        } else {
            p
        };
        let is_glob = p_clean.contains('*')
            || p_clean.contains('?')
            || p_clean.contains('[')
            || p_clean.contains('{');
        if !is_glob {
            let path = base_dir.join(p_clean);
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
        if p_clean != p
            && let Ok(g) = Glob::new(p_clean)
        {
            include_builder.add(g);
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
            roots.push(base_dir.to_path_buf());
        } else {
            roots.push(base_dir.join(root));
        }
    }

    let mut exclude_builder = GlobSetBuilder::new();
    for p in exclude_patterns {
        let p_clean = if let Some(stripped) = p.strip_prefix("./") {
            stripped
        } else {
            p
        };
        if let Ok(g) = Glob::new(p_clean) {
            exclude_builder.add(g);
        }
        if p != p_clean
            && let Ok(g) = Glob::new(p)
        {
            exclude_builder.add(g);
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
        roots.push(base_dir.to_path_buf());
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
            .follow_links(true)
            .build();

        for entry in walk.filter_map(|e| e.ok()) {
            if entry.file_type().is_some_and(|ft| ft.is_file()) {
                let path = entry.path();
                if is_relevant_file(path) {
                    let mut matched = include_set.is_match(path);

                    if !matched
                        && let Ok(abs_path) = std::fs::canonicalize(path) {
                            matched = include_set.is_match(&abs_path);
                        }

                    if !matched && let Some(rel_to_base) = pathdiff::diff_paths(path, base_dir) {
                        matched = include_set.is_match(&rel_to_base);
                    }

                    if !matched
                        && let Some(file_name) = path.file_name()
                        && include_set.is_match(file_name)
                    {
                        matched = true;
                    }

                    if matched {
                        let mut excluded = exclude_set.is_match(path);
                        if !excluded
                            && let Some(rel_to_base) = pathdiff::diff_paths(path, base_dir)
                            && exclude_set.is_match(&rel_to_base)
                        {
                            excluded = true;
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

pub fn get_gitignore_matcher(base_dir: &Path) -> ignore::gitignore::Gitignore {
    let mut builder = ignore::gitignore::GitignoreBuilder::new(base_dir);
    let gitignore_path = base_dir.join(".gitignore");
    if gitignore_path.exists() {
        builder.add(gitignore_path);
    }
    builder
        .build()
        .unwrap_or_else(|_| ignore::gitignore::Gitignore::empty())
}

pub fn is_path_ignored(path: &Path, matcher: &ignore::gitignore::Gitignore) -> bool {
    matcher.matched(path, path.is_dir()).is_ignore()
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

pub fn get_output_path(path: &Path, base_dir: &Path, output_dir: Option<&str>) -> PathBuf {
    if let Some(dir) = output_dir {
        let mut p = base_dir.join(dir);
        let rel = if path.is_absolute() {
            path.strip_prefix(base_dir).unwrap_or(path)
        } else {
            path
        };
        p.push(rel);
        p.set_extension("codegen.ts");
        p
    } else {
        let mut p = path.to_path_buf();
        p.set_extension("codegen.ts");
        p
    }
}

pub fn merge_schema_texts(texts: &[String]) -> String {
    let total_len: usize = texts.iter().map(|s| s.len() + 1).sum();
    let mut merged = String::with_capacity(total_len);
    let mut seen_base = fnv::FnvHashSet::default();

    let mut parser = tree_sitter::Parser::new();
    let language: tree_sitter::Language = tree_sitter_graphql::LANGUAGE.into();
    if let Err(e) = parser.set_language(&language) {
        eprintln!(
            "{}: Failed to set GraphQL language: {}",
            "ERROR".red(),
            e.to_string().red()
        );
        for text in texts {
            merged.push_str(text);
            merged.push('\n');
        }
        return merged;
    }

    let query = crate::queries::GQL_MERGE_QUERY_CACHE.get_or_init(|| {
        tree_sitter::Query::new(&language, crate::queries::GQL_MERGE_QUERY).unwrap()
    });
    let name_idx = query.capture_index_for_name("name").unwrap();
    let type_def_idx = query.capture_index_for_name("type_def").unwrap();
    let mut cursor = tree_sitter::QueryCursor::new();

    for text in texts {
        let tree = if let Some(t) = parser.parse(text, None) {
            t
        } else {
            merged.push_str(text);
            merged.push('\n');
            continue;
        };
        let root = tree.root_node();

        let mut matches = cursor.matches(query, root, text.as_bytes());

        let mut modifications = Vec::new();

        while let Some(m) = matches.next() {
            let mut name_node = None;
            let mut container_node = None;
            for cap in m.captures {
                if cap.index == name_idx {
                    name_node = Some(cap.node);
                } else if cap.index == type_def_idx {
                    container_node = Some(cap.node);
                }
            }

            if let (Some(name_node), Some(container_node)) = (name_node, container_node) {
                let name = &text[name_node.start_byte()..name_node.end_byte()];
                let is_extension = container_node.kind() == "type_extension";

                if !is_extension {
                    if seen_base.contains(name) {
                        let is_scalar = container_node.kind() == "scalar_type_definition";
                        let mut has_directives = false;
                        let mut cursor = container_node.walk();
                        for child in container_node.children(&mut cursor) {
                            if child.kind() == "directives" {
                                has_directives = true;
                                break;
                            }
                        }

                        if is_scalar && !has_directives {
                            // Just remove duplicate scalar with no directives as "extend scalar Name" is invalid without directives
                            modifications.push((
                                container_node.start_byte(),
                                container_node.end_byte(),
                                "".to_string(),
                            ));
                        } else {
                            // We need to convert this to an extension.
                            // We must skip any description or comments that come before the keyword.
                            let mut insert_pos = container_node.start_byte();

                            let mut cursor = container_node.walk();
                            for child in container_node.children(&mut cursor) {
                                let kind = child.kind();
                                if kind != "description" && kind != "comment" {
                                    insert_pos = child.start_byte();
                                    break;
                                }
                            }

                            // We replace the range from container start to keyword start with "extend "
                            // This effectively strips the description from the extension.
                            modifications.push((
                                container_node.start_byte(),
                                insert_pos,
                                "extend ".to_string(),
                            ));
                        }
                    } else {
                        seen_base.insert(name.to_string());
                    }
                }
            }
        }

        modifications.sort_by_key(|m| m.0);
        let mut current_pos = 0;
        for (start, end, replacement) in modifications {
            merged.push_str(&text[current_pos..start]);
            merged.push_str(&replacement);
            current_pos = end;
        }
        merged.push_str(&text[current_pos..]);
        merged.push('\n');
    }

    merged
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
    fn test_is_relevant_file() {
        assert!(is_relevant_file(Path::new("test.graphql")));
        assert!(is_relevant_file(Path::new("test.ts")));
        assert!(is_relevant_file(Path::new("src/test.tsx")));
        assert!(is_relevant_file(Path::new("graphql.ts")));

        // Should ignore generated files
        assert!(!is_relevant_file(Path::new("test.codegen.ts")));
        assert!(!is_relevant_file(Path::new("manifest.json")));
        assert!(!is_relevant_file(Path::new("permissions.ts")));

        // Should ignore common directories
        assert!(!is_relevant_file(Path::new("node_modules/test.ts")));
        assert!(!is_relevant_file(Path::new(".git/config")));
    }

    #[test]
    #[ntest::timeout(100)]
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
