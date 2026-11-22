//! A run with nothing to regenerate must leave every output's mtime alone. Build
//! caches and file watchers read a moved mtime as a change, so a file rewritten with
//! the same bytes makes everything downstream of it run again.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::support::cmd::{assert_command_succeeded, fresh_dir, graphox};

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(files_under(&path));
        } else {
            files.push(path);
        }
    }
    files
}

#[test]
#[ntest::timeout(20000)]
fn test_codegen_leaves_unchanged_outputs_untouched() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = fresh_dir("graphox_unchanged_outputs_test");

    std::fs::write(
        temp_dir.join("schema.graphql"),
        "type User { id: ID! name: String } type Query { me: User }",
    )
    .unwrap();
    std::fs::create_dir_all(temp_dir.join("src")).unwrap();
    std::fs::write(
        temp_dir.join("src/query.graphql"),
        "query Me { me { id ...UserName } }\nfragment UserName on User { name }",
    )
    .unwrap();
    // Every optional per-project file is switched on, so each one is covered.
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
enable_schema_cache: false
codegen:
  fragment_masking: enabled
  emit_permission_data: true
schema_types:
  - schema: "schema.graphql"
    output: "types/schema.ts"
    possible_types: "types/possible-types.ts"
    type_policies: "types/type-policies.ts"
projects:
  - schema: "schema.graphql"
    include: "src/**/*.graphql"
    output_dir: "generated"
"#,
    )
    .unwrap();

    let run = || {
        let output = graphox(bin_path, &temp_dir)
            .arg("codegen")
            .output()
            .expect("Failed to execute process");
        assert_command_succeeded(&output, "codegen", &temp_dir);
    };

    run();

    let mut outputs = files_under(&temp_dir.join("generated"));
    outputs.extend(files_under(&temp_dir.join("types")));
    outputs.sort();
    for name in [
        "index.ts",
        "fragment-masking.ts",
        "permissions.ts",
        "query.codegen.ts",
    ] {
        assert!(
            outputs.iter().any(|p| p.ends_with(name)),
            "expected {name} among the outputs: {outputs:?}"
        );
    }

    // Backdating every output, rather than waiting, makes any rewrite visible
    // whatever the file system's timestamp resolution.
    let backdated = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    for path in &outputs {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(backdated)
            .unwrap();
    }

    run();

    let rewritten: Vec<_> = outputs
        .iter()
        .filter(|path| std::fs::metadata(path).unwrap().modified().unwrap() != backdated)
        .collect();
    assert!(
        rewritten.is_empty(),
        "a run with nothing to regenerate rewrote {rewritten:?}"
    );
}
