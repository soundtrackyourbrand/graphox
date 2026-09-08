//! Every relative import in a generated file has to resolve to a file that
//! codegen actually wrote.
//!
//! The specifier is a path between two *generated* files, so it has to be
//! derived from where those land — not from where their sources sit. The two
//! only agree when every project puts its output at the same depth below its
//! sources, which is why this went unnoticed: the common single-project and
//! uniform-`output_dir` layouts happen to make the wrong calculation right.
//!
//! Asserting on the literal specifier is not enough on its own. `pkg_a/gen`
//! importing `../pkg_b/fragments.codegen` still contains the substring a
//! `contains("pkg_b/fragments.codegen")` check looks for, while pointing at
//! `pkg_a/pkg_b/` and resolving to nothing. So these tests resolve each
//! specifier against the filesystem.

use std::fs;
use std::path::{Path, PathBuf};

use crate::support::cmd::graphox;

/// Relative module specifiers imported by `file`, in source order.
fn relative_imports(file: &Path) -> Vec<String> {
    let content = fs::read_to_string(file)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", file.display(), e));

    content
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            if !line.starts_with("import") {
                return None;
            }
            let (_, rest) = line.split_once(" from \"")?;
            let (specifier, _) = rest.split_once('"')?;
            specifier.starts_with('.').then(|| specifier.to_string())
        })
        .collect()
}

/// Every generated `.ts` file under `root`.
fn generated_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "ts") {
                found.push(path);
            }
        }
    }

    found.sort();
    found
}

/// Assert that every relative import in every generated file under `root`
/// points at a file that exists. Returns the number of imports checked so a
/// caller can confirm it actually exercised something.
fn assert_relative_imports_resolve(root: &Path) -> usize {
    let mut checked = 0;

    for file in generated_files(root) {
        let dir = file.parent().expect("generated file has a parent");
        for specifier in relative_imports(&file) {
            // Generated imports are extensionless unless `emit_extensions` is
            // set, so try the specifier as written and with `.ts` appended.
            // Appended, not `with_extension`: the specifier already ends in
            // `.codegen`, which `with_extension` would replace.
            let target = dir.join(&specifier);
            let with_ts = {
                let mut name = target.clone().into_os_string();
                name.push(".ts");
                PathBuf::from(name)
            };
            let resolved = target.exists() || with_ts.exists();

            assert!(
                resolved,
                "{} imports \"{}\", which resolves to {} — no such file was generated",
                file.display(),
                specifier,
                target.display()
            );
            checked += 1;
        }
    }

    checked
}

fn temp_dir_for(name: &str) -> PathBuf {
    let dir = std::env::current_dir()
        .unwrap()
        .join("target")
        .join("tmp")
        .join(name);
    if dir.exists() {
        fs::remove_dir_all(&dir).ok();
    }
    fs::create_dir_all(&dir).unwrap();
    fs::canonicalize(&dir).unwrap()
}

/// Two projects nesting `output_dir` at different depths. `app` sits one level
/// down and `shared` two, so a source-relative specifier is short by a segment
/// and points into `app/` instead of at the sibling package.
#[test]
#[ntest::timeout(30000)]
fn imports_resolve_when_projects_nest_output_dir_differently() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let dir = temp_dir_for("import_paths_asymmetric_output_dir");

    fs::create_dir_all(dir.join("app")).unwrap();
    fs::create_dir_all(dir.join("shared/deep")).unwrap();

    fs::write(
        dir.join("schema.graphql"),
        "type Query { user: User } type User { id: ID! name: String }",
    )
    .unwrap();
    fs::write(
        dir.join("shared/deep/fragments.graphql"),
        "fragment SharedFrag on User @public { name }",
    )
    .unwrap();
    fs::write(
        dir.join("app/query.graphql"),
        "query AppQuery { user { ...SharedFrag } }",
    )
    .unwrap();

    fs::write(
        dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "shared/**/*.graphql"
    output_dir: "shared/build/generated"
  - schema: "schema.graphql"
    include: "app/**/*.graphql"
    output_dir: "app/gen"
"#,
    )
    .unwrap();

    let output = graphox(bin_path, &dir).arg("codegen").output().unwrap();
    assert!(
        output.status.success(),
        "codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let checked = assert_relative_imports_resolve(&dir);
    assert!(
        checked > 0,
        "no relative imports were generated, so nothing was exercised"
    );

    fs::remove_dir_all(dir).ok();
}

/// The same guard over the committed fixture that first surfaced this, which
/// adds a transitive private fragment to the picture.
#[test]
#[ntest::timeout(30000)]
fn imports_resolve_for_transitive_private_fragment_fixture() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let dir = temp_dir_for("import_paths_transitive_private_fragment");

    let fixture = Path::new("tests/fixtures/transitive_private_fragment");
    for relative in ["schema.graphql", "graphox.yaml"] {
        fs::copy(fixture.join(relative), dir.join(relative)).unwrap();
    }
    for (subdir, file) in [("app", "query.ts"), ("shared", "fragments.ts")] {
        fs::create_dir_all(dir.join(subdir)).unwrap();
        fs::copy(fixture.join(subdir).join(file), dir.join(subdir).join(file)).unwrap();
    }

    let output = graphox(bin_path, &dir).arg("codegen").output().unwrap();
    assert!(
        output.status.success(),
        "codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let checked = assert_relative_imports_resolve(&dir);
    assert!(
        checked > 0,
        "no relative imports were generated, so nothing was exercised"
    );

    fs::remove_dir_all(dir).ok();
}
