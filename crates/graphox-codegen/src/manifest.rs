use std::path::Path;

use crate::context::{FragmentGenerated, OperationGenerated};

pub const MANIFEST_FILE_NAME: &str = "manifest.json";

/// The `manifest.json` of an output directory: one entry per definition the
/// build-tool plugins can rewrite, giving its source text, its document's
/// export name and the generated module that exports it, relative to the
/// directory and without its extension. A fragment without a document, such
/// as a `@type_only` one, is listed with its source text alone, and the
/// plugins turn its `graphql()` call into `{}`, as `graphql.ts` does.
///
/// The CLI and the language server both write it, so it is built here once
/// and both write the same bytes.
pub fn generate_manifest_content(
    output_dir: &Path,
    operations: &[OperationGenerated],
    fragments: &[FragmentGenerated],
) -> String {
    let entries: Vec<_> = operations
        .iter()
        .map(|op| {
            entry(
                output_dir,
                &op.source_text,
                &op.codegen_path,
                &op.document_name,
            )
        })
        .chain(fragments.iter().map(|frag| {
            if frag.has_document {
                entry(
                    output_dir,
                    &frag.source_text,
                    &frag.codegen_path,
                    &frag.document_name,
                )
            } else {
                sonic_rs::json!({ "source": frag.source_text })
            }
        }))
        .collect();

    sonic_rs::to_string_pretty(&entries).expect("manifest entries serialize")
}

/// Writes `manifest.json` into `output_dir`, leaving it untouched when its
/// content has not changed so that watchers do not see a write.
pub fn write_manifest(
    output_dir: &Path,
    operations: &[OperationGenerated],
    fragments: &[FragmentGenerated],
) -> std::io::Result<()> {
    let path = output_dir.join(MANIFEST_FILE_NAME);
    let content = generate_manifest_content(output_dir, operations, fragments);
    if std::fs::read_to_string(&path).is_ok_and(|existing| existing == content) {
        return Ok(());
    }
    std::fs::write(&path, content)
}

fn entry(output_dir: &Path, source: &str, codegen_path: &Path, name: &str) -> sonic_rs::Value {
    let rel_path = pathdiff::diff_paths(codegen_path, output_dir)
        .unwrap_or_else(|| codegen_path.to_path_buf());
    let mut path = graphox_core::utils::to_posix_path(&rel_path);
    if !path.starts_with('.') && !path.starts_with('/') && !rel_path.is_absolute() {
        path = format!("./{}", path);
    }
    let path = path.strip_suffix(".ts").unwrap_or(&path);

    sonic_rs::json!({
        "source": source,
        "path": path,
        "name": name
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use apollo_compiler::ast::OperationType;
    use std::path::PathBuf;

    fn operation(source: &str, path: &str, document_name: &str) -> OperationGenerated {
        OperationGenerated {
            name: String::new(),
            operation_type: OperationType::Query,
            operation_type_name: String::new(),
            variables_type_name: String::new(),
            document_name: document_name.to_string(),
            hook_names: Vec::new(),
            source_text: source.to_string(),
            codegen_path: PathBuf::from(path),
            generated_bytes: 0,
            ast_bytes: 0,
            facts: None,
        }
    }

    fn fragment(
        source: &str,
        path: &str,
        document_name: &str,
        has_document: bool,
    ) -> FragmentGenerated {
        FragmentGenerated {
            name: String::new(),
            fragment_type_name: String::new(),
            source_text: source.to_string(),
            document_name: document_name.to_string(),
            has_document,
            codegen_path: PathBuf::from(path),
            generated_bytes: 0,
            ast_bytes: 0,
            facts: None,
        }
    }

    #[test]
    fn entries_are_relative_extensionless_and_keys_sorted() {
        let content = generate_manifest_content(
            Path::new("/repo/src/__generated__"),
            &[operation(
                "query A { a }",
                "/repo/src/a.codegen.ts",
                "ADocument",
            )],
            &[],
        );

        assert_eq!(
            content,
            r#"[
  {
    "name": "ADocument",
    "path": "../a.codegen",
    "source": "query A { a }"
  }
]"#
        );
    }

    #[test]
    fn a_path_inside_the_directory_starts_with_dot_slash() {
        let content = generate_manifest_content(
            Path::new("/repo/__generated__"),
            &[operation("query A { a }", "/repo/__generated__/a.ts", "A")],
            &[],
        );

        assert!(content.contains(r#""path": "./a""#), "{content}");
    }

    #[test]
    fn a_fragment_without_a_document_is_listed_by_its_source_alone() {
        let frags = [
            fragment("fragment F on A { a }", "/repo/f.codegen.ts", "F", true),
            fragment(
                "fragment T on A @type_only { a }",
                "/repo/t.codegen.ts",
                "T",
                false,
            ),
        ];

        let content = generate_manifest_content(Path::new("/repo"), &[], &frags);
        assert!(
            content.contains(r#""source": "fragment F on A { a }""#),
            "{content}"
        );
        assert!(
            content.contains(
                r#"{
    "source": "fragment T on A @type_only { a }"
  }"#
            ),
            "{content}"
        );
        assert!(!content.contains(r#""name": "T""#), "{content}");
    }

    #[test]
    fn an_unchanged_manifest_is_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        let ops = [operation("query A { a }", "/repo/a.ts", "A")];

        write_manifest(dir, &ops, &[]).unwrap();
        let path = dir.join(MANIFEST_FILE_NAME);
        let first = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        write_manifest(dir, &ops, &[]).unwrap();
        let second = std::fs::metadata(&path).unwrap().modified().unwrap();

        assert_eq!(first, second);
    }
}
