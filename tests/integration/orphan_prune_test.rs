//! Generated files whose source document is gone used to survive every run but
//! `--clean`, and kept importing symbols from the outputs that *were* regenerated —
//! so renaming a fragment broke `tsc` in a file nobody had touched.

use std::path::{Path, PathBuf};
use std::process::Command;

const SCHEMA: &str =
    "type SoundZone { id: ID! name: String } type Query { zone(id: ID!): SoundZone }";

fn setup(name: &str) -> PathBuf {
    let temp_dir = std::env::temp_dir().join(name);
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(temp_dir.join("app/components")).unwrap();
    std::fs::create_dir_all(temp_dir.join("app/routes")).unwrap();
    std::fs::write(temp_dir.join("schema.graphql"), SCHEMA).unwrap();
    temp_dir
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

fn run_codegen(dir: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_graphox"))
        .current_dir(dir)
        .arg("codegen")
        .output()
        .expect("Failed to execute process")
}

fn fragment_source(fragment_name: &str) -> String {
    format!(
        "import {{ gql }} from \"graphql-tag\";\n\
         export const frag = gql(`fragment {fragment_name} on SoundZone @public {{ id name }}`);\n"
    )
}

fn route_source(fragment_name: &str) -> String {
    format!(
        "import {{ gql }} from \"graphql-tag\";\n\
         export const q = gql(`query ZoneSettings($id: ID!) {{ zone(id: $id) {{ id ...{fragment_name} }} }}`);\n"
    )
}

#[test]
fn test_orphan_removed_when_source_loses_graphql() {
    let temp_dir = setup("graphox_orphan_prune_loses_graphql");
    write(
        &temp_dir.join("graphox.yaml"),
        "enable_schema_cache: false\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.tsx\"\n    output_dir: \"app/graphql\"\n",
    );
    write(
        &temp_dir.join("app/components/SoundZoneSettings.tsx"),
        &fragment_source("SoundZoneSettings_SoundZone"),
    );
    write(
        &temp_dir.join("app/routes/zone.settings.tsx"),
        &route_source("SoundZoneSettings_SoundZone"),
    );

    assert!(run_codegen(&temp_dir).status.success());

    let orphan = temp_dir.join("app/graphql/routes/zone.settings.codegen.ts");
    let fragment_output = temp_dir.join("app/graphql/components/SoundZoneSettings.codegen.ts");
    assert!(orphan.exists());
    assert!(fragment_output.exists());

    // The refactor that surfaced this: the fragment is renamed and the route stops
    // holding any GraphQL at all, leaving its output with no live source.
    write(
        &temp_dir.join("app/components/SoundZoneSettings.tsx"),
        &fragment_source("SoundZoneSettings_SoundZoneSettings"),
    );
    write(
        &temp_dir.join("app/routes/zone.settings.tsx"),
        "export const q = 42;\n",
    );

    assert!(run_codegen(&temp_dir).status.success());

    assert!(
        !orphan.exists(),
        "output with no source document should have been pruned"
    );
    assert!(
        fragment_output.exists(),
        "live output must survive the sweep"
    );
    // Nothing left behind still references the pre-rename symbol.
    let generated = std::fs::read_to_string(&fragment_output).unwrap();
    assert!(generated.contains("SoundZoneSettings_SoundZoneSettings"));

    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_orphan_removed_when_source_deleted() {
    let temp_dir = setup("graphox_orphan_prune_source_deleted");
    write(
        &temp_dir.join("graphox.yaml"),
        "enable_schema_cache: false\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.tsx\"\n    output_dir: \"gen\"\n",
    );
    write(
        &temp_dir.join("app/components/SoundZoneSettings.tsx"),
        &fragment_source("SoundZoneSettings_SoundZone"),
    );
    write(
        &temp_dir.join("app/routes/zone.settings.tsx"),
        &route_source("SoundZoneSettings_SoundZone"),
    );

    assert!(run_codegen(&temp_dir).status.success());
    let orphan = temp_dir.join("gen/routes/zone.settings.codegen.ts");
    assert!(orphan.exists());

    std::fs::remove_file(temp_dir.join("app/routes/zone.settings.tsx")).unwrap();
    assert!(run_codegen(&temp_dir).status.success());

    assert!(
        !orphan.exists(),
        "deleted source should take its output with it"
    );
    assert!(
        temp_dir
            .join("gen/components/SoundZoneSettings.codegen.ts")
            .exists()
    );
    assert!(temp_dir.join("gen/graphql.ts").exists());
    assert!(temp_dir.join("gen/manifest.json").exists());

    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_orphan_prune_is_idempotent() {
    let temp_dir = setup("graphox_orphan_prune_idempotent");
    write(
        &temp_dir.join("graphox.yaml"),
        "enable_schema_cache: false\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.tsx\"\n    output_dir: \"gen\"\n",
    );
    write(
        &temp_dir.join("app/components/SoundZoneSettings.tsx"),
        &fragment_source("SoundZoneSettings_SoundZone"),
    );
    write(
        &temp_dir.join("app/routes/zone.settings.tsx"),
        &route_source("SoundZoneSettings_SoundZone"),
    );

    assert!(run_codegen(&temp_dir).status.success());
    let outputs = [
        temp_dir.join("gen/components/SoundZoneSettings.codegen.ts"),
        temp_dir.join("gen/routes/zone.settings.codegen.ts"),
    ];
    assert!(outputs.iter().all(|p| p.exists()));

    // A second run has an identical source set and must not remove anything.
    assert!(run_codegen(&temp_dir).status.success());
    for output in &outputs {
        assert!(
            output.exists(),
            "{} should survive a no-op run",
            output.display()
        );
    }

    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_orphan_prune_skipped_when_project_fails() {
    let temp_dir = setup("graphox_orphan_prune_project_fails");
    write(
        &temp_dir.join("graphox.yaml"),
        "enable_schema_cache: false\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.tsx\"\n    output_dir: \"gen\"\n",
    );
    write(
        &temp_dir.join("app/components/SoundZoneSettings.tsx"),
        &fragment_source("SoundZoneSettings_SoundZone"),
    );
    write(
        &temp_dir.join("app/routes/zone.settings.tsx"),
        &route_source("SoundZoneSettings_SoundZone"),
    );

    assert!(run_codegen(&temp_dir).status.success());
    let output = temp_dir.join("gen/routes/zone.settings.codegen.ts");
    assert!(output.exists());

    // Delete one source and break another: the project can't produce a complete
    // keep-set, so nothing may be swept against it.
    std::fs::remove_file(temp_dir.join("app/routes/zone.settings.tsx")).unwrap();
    write(
        &temp_dir.join("app/components/SoundZoneSettings.tsx"),
        "import { gql } from \"graphql-tag\";\nexport const frag = gql(`fragment F on SoundZone { nope }`);\n",
    );

    assert!(!run_codegen(&temp_dir).status.success());
    assert!(
        output.exists(),
        "a failed run must not prune against an incomplete keep-set"
    );

    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_orphan_prune_can_be_disabled() {
    let temp_dir = setup("graphox_orphan_prune_disabled");
    write(
        &temp_dir.join("graphox.yaml"),
        "enable_schema_cache: false\ncodegen:\n  prune_orphans: false\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.tsx\"\n    output_dir: \"gen\"\n",
    );
    write(
        &temp_dir.join("app/components/SoundZoneSettings.tsx"),
        &fragment_source("SoundZoneSettings_SoundZone"),
    );
    write(
        &temp_dir.join("app/routes/zone.settings.tsx"),
        &route_source("SoundZoneSettings_SoundZone"),
    );

    assert!(run_codegen(&temp_dir).status.success());
    let output = temp_dir.join("gen/routes/zone.settings.codegen.ts");
    assert!(output.exists());

    std::fs::remove_file(temp_dir.join("app/routes/zone.settings.tsx")).unwrap();
    assert!(run_codegen(&temp_dir).status.success());
    assert!(
        output.exists(),
        "prune_orphans: false must keep the previous behaviour"
    );

    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_orphan_prune_shared_output_dir_with_disabled_project() {
    let temp_dir = setup("graphox_orphan_prune_shared_output_dir");
    std::fs::create_dir_all(temp_dir.join("b")).unwrap();
    write(
        &temp_dir.join("graphox.yaml"),
        "enable_schema_cache: false\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.tsx\"\n    output_dir: \"gen\"\n  - schema: \"schema.graphql\"\n    include: \"b/**/*.tsx\"\n    output_dir: \"gen\"\n",
    );
    write(
        &temp_dir.join("app/components/SoundZoneSettings.tsx"),
        &fragment_source("SoundZoneSettings_SoundZone"),
    );
    write(
        &temp_dir.join("b/Other.tsx"),
        "import { gql } from \"graphql-tag\";\nexport const q = gql(`query Other { zone(id: \"1\") { name } }`);\n",
    );

    assert!(run_codegen(&temp_dir).status.success());
    let other_output = temp_dir.join("gen/Other.codegen.ts");
    assert!(other_output.exists());

    // The second project stops running, so its outputs are outside this run's
    // keep-set — the shared directory must not be swept.
    write(
        &temp_dir.join("graphox.yaml"),
        "enable_schema_cache: false\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.tsx\"\n    output_dir: \"gen\"\n  - schema: \"schema.graphql\"\n    include: \"b/**/*.tsx\"\n    output_dir: \"gen\"\n    codegen:\n      enabled: false\n",
    );

    assert!(run_codegen(&temp_dir).status.success());
    assert!(
        other_output.exists(),
        "a directory shared with a project that didn't run must be left alone"
    );

    std::fs::remove_dir_all(temp_dir).ok();
}

#[test]
fn test_orphan_prune_without_output_dir() {
    let temp_dir = setup("graphox_orphan_prune_colocated");
    write(
        &temp_dir.join("graphox.yaml"),
        "enable_schema_cache: false\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.tsx\"\n",
    );
    write(
        &temp_dir.join("app/components/SoundZoneSettings.tsx"),
        &fragment_source("SoundZoneSettings_SoundZone"),
    );
    write(
        &temp_dir.join("app/components/Other.tsx"),
        "import { gql } from \"graphql-tag\";\nexport const q = gql(`query Other { zone(id: \"1\") { name } }`);\n",
    );

    assert!(run_codegen(&temp_dir).status.success());
    let orphan = temp_dir.join("components/Other.codegen.ts");
    let live = temp_dir.join("components/SoundZoneSettings.codegen.ts");
    assert!(
        orphan.exists(),
        "co-located output should exist: {:?}",
        orphan
    );
    assert!(live.exists());

    std::fs::remove_file(temp_dir.join("app/components/Other.tsx")).unwrap();
    assert!(run_codegen(&temp_dir).status.success());

    assert!(!orphan.exists(), "co-located orphan should be pruned");
    assert!(live.exists(), "co-located live output must survive");

    std::fs::remove_dir_all(temp_dir).ok();
}

/// Output paths mirror the source tree, so a project's writes are not confined to the
/// `output_dir` it declares: with a nested `output_dir`, the outer project writes into
/// the inner project's directory. A per-directory keep-set makes the inner sweep delete
/// the outer project's live output — generated and deleted in the same run.
#[test]
fn test_nested_output_dirs_do_not_delete_each_others_outputs() {
    let temp_dir = setup("graphox_orphan_prune_nested_dirs");
    write(
        &temp_dir.join("graphox.yaml"),
        "enable_schema_cache: false\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"src/a/**/*.graphql\"\n    output_dir: \"gen\"\n  - schema: \"schema.graphql\"\n    include: \"src/b/**/*.graphql\"\n    output_dir: \"gen/nested\"\n",
    );
    write(
        &temp_dir.join("src/a/nested/Foo.graphql"),
        "query Foo { zone(id: \"1\") { id } }",
    );
    write(
        &temp_dir.join("src/b/Bar.graphql"),
        "query Bar { zone(id: \"2\") { name } }",
    );

    assert!(run_codegen(&temp_dir).status.success());

    let outer = temp_dir.join("gen/nested/Foo.codegen.ts");
    let inner = temp_dir.join("gen/nested/Bar.codegen.ts");
    assert!(
        outer.exists(),
        "the outer project's output lands in the inner project's dir and must survive it"
    );
    assert!(inner.exists());

    // And it must still be there after a second run, not resurrected-then-deleted.
    assert!(run_codegen(&temp_dir).status.success());
    assert!(outer.exists());
    assert!(inner.exists());

    std::fs::remove_dir_all(temp_dir).ok();
}

/// A project that didn't run protects its outputs wherever they landed — including
/// inside a live project's `output_dir`, i.e. when the blocked dir is an *ancestor*.
#[test]
fn test_blocked_project_protects_outputs_under_a_live_dir() {
    let temp_dir = setup("graphox_orphan_prune_blocked_ancestor");
    write(
        &temp_dir.join("graphox.yaml"),
        "enable_schema_cache: false\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"src/a/**/*.graphql\"\n    output_dir: \"gen\"\n  - schema: \"schema.graphql\"\n    include: \"src/b/**/*.graphql\"\n    output_dir: \"gen/nested\"\n",
    );
    write(
        &temp_dir.join("src/a/nested/Foo.graphql"),
        "query Foo { zone(id: \"1\") { id } }",
    );
    write(
        &temp_dir.join("src/b/Bar.graphql"),
        "query Bar { zone(id: \"2\") { name } }",
    );

    assert!(run_codegen(&temp_dir).status.success());
    let outer = temp_dir.join("gen/nested/Foo.codegen.ts");
    assert!(outer.exists());

    // Disable the outer project: its keep-set is now unknown, and its outputs sit
    // under the still-live inner project's sweep root.
    write(
        &temp_dir.join("graphox.yaml"),
        "enable_schema_cache: false\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"src/a/**/*.graphql\"\n    output_dir: \"gen\"\n    codegen:\n      enabled: false\n  - schema: \"schema.graphql\"\n    include: \"src/b/**/*.graphql\"\n    output_dir: \"gen/nested\"\n",
    );

    assert!(run_codegen(&temp_dir).status.success());
    assert!(
        outer.exists(),
        "a blocked project's outputs must survive a live sweep of the dir containing them"
    );

    std::fs::remove_dir_all(temp_dir).ok();
}

/// A project with no `output_dir` writes under `base_dir` mirroring the path below its
/// include root, which can land inside another project's `output_dir`. Neither sweep
/// may treat the other's output as an orphan.
#[test]
fn test_colocated_project_writing_into_another_projects_output_dir() {
    let temp_dir = setup("graphox_orphan_prune_colocated_overlap");
    write(
        &temp_dir.join("graphox.yaml"),
        "enable_schema_cache: false\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.graphql\"\n  - schema: \"schema.graphql\"\n    include: \"lib/**/*.graphql\"\n    output_dir: \"shared\"\n",
    );
    write(
        &temp_dir.join("app/shared/A.graphql"),
        "query A { zone(id: \"1\") { id } }",
    );
    write(
        &temp_dir.join("lib/B.graphql"),
        "query B { zone(id: \"2\") { name } }",
    );

    assert!(run_codegen(&temp_dir).status.success());

    let colocated = temp_dir.join("shared/A.codegen.ts");
    let with_output_dir = temp_dir.join("shared/B.codegen.ts");
    assert!(
        colocated.exists(),
        "co-located output must survive the other project's sweep"
    );
    assert!(
        with_output_dir.exists(),
        "output_dir output must survive the co-located sweep"
    );

    assert!(run_codegen(&temp_dir).status.success());
    assert!(colocated.exists());
    assert!(with_output_dir.exists());

    std::fs::remove_dir_all(temp_dir).ok();
}

/// A source that exists but can't be read is missing from the document set, which is
/// exactly how "holds no GraphQL" looks. Treating it as a deletion would delete the
/// output of a file that is still on disk.
#[test]
#[cfg(unix)]
fn test_unreadable_source_blocks_pruning() {
    use std::os::unix::fs::PermissionsExt;

    let temp_dir = setup("graphox_orphan_prune_unreadable");
    write(
        &temp_dir.join("graphox.yaml"),
        "enable_schema_cache: false\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.graphql\"\n    output_dir: \"gen\"\n",
    );
    write(
        &temp_dir.join("app/A.graphql"),
        "query A { zone(id: \"1\") { id } }",
    );
    write(
        &temp_dir.join("app/B.graphql"),
        "query B { zone(id: \"2\") { name } }",
    );

    assert!(run_codegen(&temp_dir).status.success());
    let a_out = temp_dir.join("gen/A.codegen.ts");
    let b_out = temp_dir.join("gen/B.codegen.ts");
    assert!(a_out.exists() && b_out.exists());

    let unreadable = temp_dir.join("app/B.graphql");
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();
    let output = run_codegen(&temp_dir);
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o644)).unwrap();

    assert!(
        b_out.exists(),
        "the unreadable file's output must not be treated as an orphan"
    );
    assert!(
        a_out.exists(),
        "nothing else in the project may be swept against an incomplete keep-set"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unreadable"),
        "the skipped cleanup should be reported, got: {stderr}"
    );

    std::fs::remove_dir_all(temp_dir).ok();
}

/// `has_generated_header` decides whether a file is graphox's own output and therefore
/// not a source. A hand-written file that merely mentions the marker sentence must not
/// be misread as generated — that would drop it from the scan and prune its output.
#[test]
fn test_source_mentioning_generated_marker_keeps_its_output() {
    let temp_dir = setup("graphox_orphan_prune_generated_marker");
    write(
        &temp_dir.join("graphox.yaml"),
        "enable_schema_cache: false\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.tsx\"\n    output_dir: \"gen\"\n",
    );
    write(
        &temp_dir.join("app/A.tsx"),
        "import { gql } from \"graphql-tag\";\nexport const q = gql(`query A { zone(id: \"1\") { id } }`);\n",
    );
    write(
        &temp_dir.join("app/B.tsx"),
        "import { gql } from \"graphql-tag\";\nexport const q = gql(`query B { zone(id: \"2\") { name } }`);\n",
    );

    assert!(run_codegen(&temp_dir).status.success());
    let b_out = temp_dir.join("gen/B.codegen.ts");
    assert!(b_out.exists());

    write(
        &temp_dir.join("app/B.tsx"),
        "import { gql } from \"graphql-tag\";\n// note: files under gen/ say \"This file was automatically generated and should not be edited.\"\nexport const q = gql(`query B { zone(id: \"2\") { name } }`);\n",
    );

    assert!(run_codegen(&temp_dir).status.success());
    assert!(
        b_out.exists(),
        "a source that mentions the generated-header sentence is still a source"
    );

    std::fs::remove_dir_all(temp_dir).ok();
}

/// When `output_dir` doubles as an include root the directory tree belongs to the user:
/// orphans are still swept, but directories emptied by the sweep are left standing.
#[test]
fn test_surgical_output_dir_prunes_files_but_keeps_directories() {
    let temp_dir = setup("graphox_orphan_prune_surgical");
    write(
        &temp_dir.join("graphox.yaml"),
        "enable_schema_cache: false\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"src/**/*.graphql\"\n    output_dir: \"src\"\n",
    );
    write(
        &temp_dir.join("src/keep/A.graphql"),
        "query A { zone(id: \"1\") { id } }",
    );
    write(
        &temp_dir.join("src/deep/B.graphql"),
        "query B { zone(id: \"2\") { name } }",
    );

    assert!(run_codegen(&temp_dir).status.success());
    let orphan = temp_dir.join("src/deep/B.codegen.ts");
    assert!(orphan.exists());

    std::fs::remove_file(temp_dir.join("src/deep/B.graphql")).unwrap();
    assert!(run_codegen(&temp_dir).status.success());

    assert!(
        !orphan.exists(),
        "orphan should be pruned in the surgical case too"
    );
    assert!(
        temp_dir.join("src/deep").exists(),
        "the directory tree is the user's here and must be left standing"
    );
    assert!(temp_dir.join("src/keep/A.codegen.ts").exists());

    std::fs::remove_dir_all(temp_dir).ok();
}
