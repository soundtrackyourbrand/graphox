//! `graphql-facts.json`, written by codegen when `emit_facts` is on. Other
//! tools read it, so its shape is pinned here in full.

use graphox::{
    Config, config::CodegenConfig, config::GlobPattern, config::ProjectConfig, config::SchemaSource,
};
use graphox_cli::run_codegen;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

const SCHEMA: &str = r#"
type Query { playlist(id: ID!): Playlist }
type Mutation { addTracks(id: ID!, uris: [String!]!): Playlist }
type Playlist { id: ID! name: String tracks(first: Int, market: String): TrackConnection! }
type TrackConnection { edges: [TrackEdge!]! }
type TrackEdge { cursor: String! node: Track! }
type Track { id: ID! title: String previewUrl: String }
"#;

fn config(base_dir: &Path, emit_facts: bool) -> Config {
    Config::new_test(
        base_dir.to_path_buf(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("src/**/*.graphql".to_string()))
                .with_output_dir("src/__generated__".to_string())
                .with_codegen(CodegenConfig::default().with_emit_facts(emit_facts)),
        ],
    )
}

fn workspace(documents: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("schema.graphql"), SCHEMA).unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    for (name, text) in documents {
        fs::write(dir.path().join("src").join(name), text).unwrap();
    }
    dir
}

fn facts_path(base_dir: &Path) -> std::path::PathBuf {
    base_dir.join("src/__generated__/graphql-facts.json")
}

#[tokio::test]
#[ntest::timeout(30000)]
async fn facts_pin_the_format() {
    let dir = workspace(&[
        (
            "page.graphql",
            "query Page($id: ID!, $first: Int = 100) { playlist(id: $id) { \
             t: tracks(first: $first, market: \"SE\") { edges { cursor node { ...Row } } } } }\n",
        ),
        (
            "row.graphql",
            "fragment Row on Track { id ... on Track @include(if: true) { title } }\n",
        ),
        (
            "add.graphql",
            "mutation Add($id: ID!) { addTracks(id: $id, uris: [\"a\", \"b\"]) { id } }\n",
        ),
    ]);
    let base_dir = dir.path().canonicalize().unwrap();

    run_codegen(config(&base_dir, true), false, false, false, None).await;

    let facts = fs::read_to_string(facts_path(&base_dir)).unwrap();
    let expected = r#"{"version":1,"definitions":[
{"document":"AddMutationDocument","kind":"mutation","name":"Add","selections":[{"arguments":[{"name":"id","value":{"kind":"variable","name":"id"}},{"name":"uris","value":{"kind":"list","values":[{"kind":"string","value":"a"},{"kind":"string","value":"b"}]}}],"kind":"field","name":"addTracks","parent":"Mutation","selections":[{"kind":"field","name":"id","parent":"Playlist","type":"ID!"}],"type":"Playlist"}],"source":"mutation Add($id: ID!) { addTracks(id: $id, uris: [\"a\", \"b\"]) { id } }\n","type":"Mutation","variables":[{"name":"id","type":"ID!"}]},
{"document":"PageQueryDocument","kind":"query","name":"Page","selections":[{"arguments":[{"name":"id","value":{"kind":"variable","name":"id"}}],"kind":"field","name":"playlist","parent":"Query","selections":[{"alias":"t","arguments":[{"name":"first","value":{"kind":"variable","name":"first"}},{"name":"market","value":{"kind":"string","value":"SE"}}],"kind":"field","name":"tracks","parent":"Playlist","selections":[{"kind":"field","name":"edges","parent":"TrackConnection","selections":[{"kind":"field","name":"cursor","parent":"TrackEdge","type":"String!"},{"kind":"field","name":"node","parent":"TrackEdge","selections":[{"fragment":"Row","kind":"spread","on":"Track","selections":[{"kind":"field","name":"id","parent":"Track","type":"ID!"},{"directives":[{"arguments":[{"name":"if","value":{"kind":"boolean","value":true}}],"name":"include"}],"kind":"inline","on":"Track","selections":[{"kind":"field","name":"title","parent":"Track","type":"String"}]}]}],"type":"Track!"}],"type":"[TrackEdge!]!"}],"type":"TrackConnection!"}],"type":"Playlist"}],"source":"query Page($id: ID!, $first: Int = 100) { playlist(id: $id) { t: tracks(first: $first, market: \"SE\") { edges { cursor node { ...Row } } } } }\n","type":"Query","variables":[{"name":"id","type":"ID!"},{"default":{"kind":"int","value":"100"},"name":"first","type":"Int"}]},
{"document":"RowDocument","kind":"fragment","name":"Row","selections":[{"kind":"field","name":"id","parent":"Track","type":"ID!"},{"directives":[{"arguments":[{"name":"if","value":{"kind":"boolean","value":true}}],"name":"include"}],"kind":"inline","on":"Track","selections":[{"kind":"field","name":"title","parent":"Track","type":"String"}]}],"source":"fragment Row on Track { id ... on Track @include(if: true) { title } }\n","type":"Track"}
]}
"#;
    assert_eq!(facts, expected);
}

#[tokio::test]
#[ntest::timeout(30000)]
async fn facts_are_written_only_when_enabled_and_removed_when_turned_off() {
    let dir = workspace(&[("me.graphql", "query P { playlist(id: \"1\") { id } }\n")]);
    let base_dir = dir.path().canonicalize().unwrap();

    run_codegen(config(&base_dir, false), false, false, false, None).await;
    assert!(base_dir.join("src/__generated__/manifest.json").exists());
    assert!(!facts_path(&base_dir).exists());

    run_codegen(config(&base_dir, true), false, false, false, None).await;
    assert!(facts_path(&base_dir).exists());

    run_codegen(config(&base_dir, false), false, false, false, None).await;
    assert!(
        !facts_path(&base_dir).exists(),
        "turning emit_facts off should remove the file it wrote"
    );
}

#[tokio::test]
#[ntest::timeout(30000)]
async fn facts_keep_every_definition_of_a_shared_document() {
    let dir = workspace(&[
        (
            "two.graphql",
            "fragment A on Track { id }\nfragment B on Track { title }\n",
        ),
        ("q.graphql", "query Q { playlist(id: \"1\") { id } }\n"),
    ]);
    let base_dir = dir.path().canonicalize().unwrap();

    run_codegen(config(&base_dir, true), false, false, false, None).await;

    let facts = fs::read_to_string(facts_path(&base_dir)).unwrap();
    let shared = "fragment A on Track { id }\\nfragment B on Track { title }\\n";
    assert_eq!(
        facts.matches(shared).count(),
        2,
        "both fragments of two.graphql should be listed with its source. facts={facts}"
    );
}
