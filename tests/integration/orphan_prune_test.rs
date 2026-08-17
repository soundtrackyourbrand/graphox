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
        "projects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.tsx\"\n    output_dir: \"app/graphql\"\n",
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
        "projects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.tsx\"\n    output_dir: \"gen\"\n",
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
        "projects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.tsx\"\n    output_dir: \"gen\"\n",
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
        "projects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.tsx\"\n    output_dir: \"gen\"\n",
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
        "codegen:\n  prune_orphans: false\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.tsx\"\n    output_dir: \"gen\"\n",
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
        "projects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.tsx\"\n    output_dir: \"gen\"\n  - schema: \"schema.graphql\"\n    include: \"b/**/*.tsx\"\n    output_dir: \"gen\"\n",
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
        "projects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.tsx\"\n    output_dir: \"gen\"\n  - schema: \"schema.graphql\"\n    include: \"b/**/*.tsx\"\n    output_dir: \"gen\"\n    codegen:\n      enabled: false\n",
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
        "projects:\n  - schema: \"schema.graphql\"\n    include: \"app/**/*.tsx\"\n",
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
