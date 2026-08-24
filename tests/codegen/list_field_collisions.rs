use std::process::Command;

/// A field selected both inline and by a spread fragment becomes an intersection
/// of the two property types. For an object that is harmless, but a list becomes
/// `Array<A> & Array<B>`, where `.map` binds only the first constituent and the
/// other's fields silently disappear from the callback parameter. Those keys are
/// generated as one merged property instead.
fn run_codegen(name: &str, schema: &str, document: &str) -> String {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let temp_dir = std::env::temp_dir().join(format!("graphox_list_collision_{}", name));
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).unwrap();

    std::fs::write(temp_dir.join("schema.graphql"), schema).unwrap();
    std::fs::write(temp_dir.join("doc.graphql"), document).unwrap();
    std::fs::write(
        temp_dir.join("graphox.yaml"),
        r#"
projects:
  - schema: "schema.graphql"
    include: "doc.graphql"
    output_dir: "generated"
"#,
    )
    .unwrap();

    let output = Command::new(bin_path)
        .arg("codegen")
        .current_dir(&temp_dir)
        .output()
        .expect("failed to run codegen");
    assert!(
        output.status.success(),
        "codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    std::fs::read_to_string(temp_dir.join("generated/doc.codegen.ts")).expect("generated file")
}

const SCHEMA: &str = r#"
type Query { zone: Zone }
type Zone { id: ID!, edges: [Edge!]!, settings: Settings }
type Edge { cursor: String, node: Node }
type Node { id: ID!, title: String }
type Settings { volume: Int, mode: String }
"#;

#[test]
fn test_list_selected_inline_and_by_a_fragment_is_merged() {
    let generated = run_codegen(
        "inline_and_fragment",
        SCHEMA,
        r#"
query GetZone {
  zone {
    id
    edges { cursor }
    ...ZoneEdges
  }
}

fragment ZoneEdges on Zone {
  edges { node { id } }
}
"#,
    );

    let query_type = generated
        .split("export interface ZoneEdges")
        .next()
        .unwrap();

    // One property carrying both contributors' fields...
    assert!(
        query_type.contains("cursor") && query_type.contains("node"),
        "merged element should have both sides:\n{}",
        query_type
    );
    // ...and the fragment no longer supplying it, or the intersection is back.
    assert!(
        query_type.contains("Omit<ZoneEdges, 'edges'>"),
        "fragment should omit the merged key:\n{}",
        query_type
    );
}

#[test]
fn test_list_selected_by_two_fragments_is_merged() {
    let generated = run_codegen(
        "two_fragments",
        SCHEMA,
        r#"
query GetZone {
  zone {
    id
    ...ZoneCursors
    ...ZoneNodes
  }
}

fragment ZoneCursors on Zone {
  edges { cursor }
}

fragment ZoneNodes on Zone {
  edges { node { id } }
}
"#,
    );

    let query_type = generated
        .split("export interface ZoneCursors")
        .next()
        .unwrap();

    assert!(
        query_type.contains("cursor") && query_type.contains("node"),
        "merged element should have both fragments' fields:\n{}",
        query_type
    );
    assert!(
        query_type.contains("Omit<ZoneCursors, 'edges'>")
            && query_type.contains("Omit<ZoneNodes, 'edges'>"),
        "both fragments should omit the merged key:\n{}",
        query_type
    );
}

#[test]
fn test_singular_field_collision_is_left_as_an_intersection() {
    // `{ volume } & { mode }` reaches both members, so nothing is rewritten and
    // the generated shape stays as it was.
    let generated = run_codegen(
        "singular",
        SCHEMA,
        r#"
query GetZone {
  zone {
    id
    settings { volume }
    ...ZoneSettings
  }
}

fragment ZoneSettings on Zone {
  settings { mode }
}
"#,
    );

    let query_type = generated
        .split("export interface ZoneSettings")
        .next()
        .unwrap();

    assert!(
        !query_type.contains("Omit<"),
        "singular collisions should not be rewritten:\n{}",
        query_type
    );
    assert!(
        query_type.contains("& ZoneSettings"),
        "fragment should still be intersected whole:\n{}",
        query_type
    );
}

#[test]
fn test_list_without_a_collision_is_unchanged() {
    let generated = run_codegen(
        "no_collision",
        SCHEMA,
        r#"
query GetZone {
  zone {
    id
    ...ZoneEdges
  }
}

fragment ZoneEdges on Zone {
  edges { node { id } }
}
"#,
    );

    let query_type = generated
        .split("export interface ZoneEdges")
        .next()
        .unwrap();

    assert!(
        !query_type.contains("Omit<") && query_type.contains("& ZoneEdges"),
        "a list only one side selects needs no merging:\n{}",
        query_type
    );
}
