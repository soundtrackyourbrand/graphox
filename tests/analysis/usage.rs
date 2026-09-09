use apollo_compiler::Schema;
use apollo_compiler::validation::Valid;
use graphox::features::analysis::DocumentSource;
use graphox::features::analysis::usage::{self, Usage};
use std::path::Path;

const SCHEMA: &str = r#"
type Query {
  account(id: ID!): Account
  zone(id: ID!): SoundZone
}
type Account {
  id: ID!
  name: String
  legacyFlag: Boolean
  owner: User
}
type User {
  id: ID!
  name: String
  email: String
}
type SoundZone {
  id: ID!
  name: String
  device: Device
}
type Device {
  id: ID!
  serial: String
}
"#;

fn schema() -> Valid<Schema> {
    Schema::parse_and_validate(SCHEMA, "schema.graphql").expect("test schema should be valid")
}

fn analyze(sources: &[(usize, &str)]) -> Usage {
    let schema = schema();
    let paths: Vec<String> = (0..sources.len())
        .map(|i| format!("doc{i}.graphql"))
        .collect();
    let docs: Vec<DocumentSource<'_>> = sources
        .iter()
        .enumerate()
        .map(|(i, (project_idx, source))| DocumentSource {
            path: Path::new(&paths[i]),
            project_idx: *project_idx,
            source,
        })
        .collect();
    usage::analyze(&schema, &docs)
}

fn consumers(usage: &Usage, type_name: &str, field: &str) -> usize {
    usage
        .fields
        .iter()
        .find(|f| f.type_name == type_name && f.field_name == field)
        .map(|f| f.consumers.len())
        .unwrap_or_else(|| panic!("no row for {type_name}.{field}"))
}

#[test]
fn counts_a_consumer_once_per_definition() {
    let usage = analyze(&[
        (0, "query A { account(id: 1) { id name } }"),
        (0, "query B { account(id: 2) { id } }"),
    ]);

    assert_eq!(consumers(&usage, "Account", "id"), 2);
    assert_eq!(consumers(&usage, "Account", "name"), 1);
}

#[test]
fn selecting_a_field_twice_in_one_definition_is_one_consumer() {
    let usage = analyze(&[(
        0,
        "query A { first: account(id: 1) { id } second: account(id: 2) { id } }",
    )]);

    assert_eq!(consumers(&usage, "Account", "id"), 1);
}

#[test]
fn a_declared_field_nothing_selects_is_reported_with_no_consumers() {
    let usage = analyze(&[(0, "query A { account(id: 1) { id } }")]);

    let row = usage
        .fields
        .iter()
        .find(|f| f.type_name == "Account" && f.field_name == "legacyFlag")
        .expect("a declared field should have a row even when unused");
    assert!(row.is_unused());
}

#[test]
fn a_fragment_is_the_consumer_of_its_own_body() {
    // The definition you would edit to stop selecting a field is the one that
    // names it, so a spread does not make the operation a consumer.
    let usage = analyze(&[
        (
            0,
            "fragment ZoneCard on SoundZone { name device { serial } }",
        ),
        (0, "query A { zone(id: 1) { id ...ZoneCard } }"),
    ]);

    assert_eq!(consumers(&usage, "SoundZone", "name"), 1);
    assert_eq!(consumers(&usage, "Device", "serial"), 1);
    assert_eq!(consumers(&usage, "SoundZone", "id"), 1);

    let name_row = usage
        .fields
        .iter()
        .find(|f| f.type_name == "SoundZone" && f.field_name == "name")
        .unwrap();
    assert_eq!(usage.definitions[name_row.consumers[0]].name, "ZoneCard");
}

#[test]
fn usage_is_recorded_against_the_type_that_declares_the_field() {
    let usage = analyze(&[(0, "query A { account(id: 1) { owner { email } } }")]);

    assert_eq!(consumers(&usage, "User", "email"), 1);
    assert_eq!(consumers(&usage, "Account", "owner"), 1);
}

#[test]
fn a_field_records_the_projects_its_consumers_belong_to() {
    let usage = analyze(&[
        (0, "query A { account(id: 1) { name } }"),
        (1, "query B { account(id: 2) { name } }"),
        (1, "query C { account(id: 3) { name } }"),
    ]);

    let row = usage
        .fields
        .iter()
        .find(|f| f.type_name == "Account" && f.field_name == "name")
        .unwrap();
    assert_eq!(row.consumers.len(), 3);
    assert_eq!(row.projects.iter().copied().collect::<Vec<_>>(), vec![0, 1]);
}

#[test]
fn types_are_ranked_by_consumer_count() {
    let usage = analyze(&[
        (0, "query A { account(id: 1) { id name } }"),
        (0, "query B { account(id: 2) { id } }"),
        (0, "query C { zone(id: 1) { id } }"),
    ]);

    let account = usage.types.iter().find(|t| t.name == "Account").unwrap();
    let zone = usage.types.iter().find(|t| t.name == "SoundZone").unwrap();
    assert_eq!(account.consumers.len(), 2);
    assert_eq!(zone.consumers.len(), 1);

    // Account before SoundZone, and both before anything unused.
    let ranked: Vec<&str> = usage
        .types
        .iter()
        .filter(|t| !t.consumers.is_empty())
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(ranked.first(), Some(&"Query"));
    assert!(
        ranked.iter().position(|n| *n == "Account") < ranked.iter().position(|n| *n == "SoundZone")
    );
}

#[test]
fn a_type_counts_the_fields_nothing_selects() {
    let usage = analyze(&[(0, "query A { account(id: 1) { id name } }")]);

    let account = usage.types.iter().find(|t| t.name == "Account").unwrap();
    assert_eq!(account.declared_fields, 4);
    // `legacyFlag` and `owner` are declared and unselected.
    assert_eq!(account.unused_fields, 2);
}

#[test]
fn fields_of_returns_one_row_per_declared_field() {
    let usage = analyze(&[(0, "query A { account(id: 1) { id } }")]);

    let mut names: Vec<&str> = usage
        .fields_of("Account")
        .map(|f| f.field_name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(names, vec!["id", "legacyFlag", "name", "owner"]);
}
