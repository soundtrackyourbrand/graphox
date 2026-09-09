use apollo_compiler::Schema;
use apollo_compiler::validation::Valid;
use graphox::config::{RepeatedSelectionKind, RepeatedSelectionsRule, Severity};
use graphox::features::analysis::repeated_selections::{
    self, Analysis, DocumentSource, Options, OverlapKind, Scope,
};
use std::path::Path;

const SCHEMA: &str = r#"
type Query {
  account(id: ID!): Account
  playlist(id: ID!): Playlist
  location(id: ID!): Location
}
type Account {
  id: ID!
  name: String
  permissions: [String!]
  address: Address
  owner: User
}
type Address {
  line1: String
  city: String
  postalCode: String
  country: String
}
type User {
  id: ID!
  name: String
  email: String
  permissions: [String!]
}
type Playlist {
  id: ID!
  name: String
  description: String
  trackCount(market: String): Int
}
type Location {
  id: ID!
  name: String
  address: Address
}
"#;

fn schema() -> Valid<Schema> {
    Schema::parse_and_validate(SCHEMA, "schema.graphql").expect("test schema should be valid")
}

/// Runs the analysis over sources given as `(project_idx, graphql)`.
fn analyze_with(opts: Options, sources: &[(usize, &str)]) -> Analysis {
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
    repeated_selections::analyze(&schema, &docs, &opts)
}

/// The mandated-field set a single project would contribute.
fn mandated_for_project(
    project: usize,
    fields: &[&str],
) -> ahash::AHashMap<usize, ahash::AHashSet<String>> {
    let mut map = ahash::AHashMap::default();
    map.insert(
        project,
        fields
            .iter()
            .map(|f| f.to_string())
            .collect::<ahash::AHashSet<_>>(),
    );
    map
}

fn analyze(sources: &[(usize, &str)]) -> Analysis {
    analyze_with(
        Options {
            min_fields: 2,
            min_uses: 2,
            ..Default::default()
        },
        sources,
    )
}

#[test]
fn reports_a_selection_that_duplicates_an_existing_fragment() {
    let analysis = analyze(&[
        (0, "fragment UserCard on User { name email }"),
        (0, "query A { account(id: 1) { owner { name email } } }"),
    ]);

    let matches: Vec<_> = analysis
        .overlaps
        .iter()
        .filter(|o| o.kind == OverlapKind::Matches)
        .collect();
    assert_eq!(matches.len(), 1, "{:#?}", analysis.overlaps);
    assert_eq!(matches[0].fragment, "UserCard");
    assert_eq!(matches[0].type_name, "User");
    assert_eq!(analysis.definitions[matches[0].site.definition].name, "A");
}

#[test]
fn reports_a_selection_that_extends_an_existing_fragment() {
    let analysis = analyze(&[
        (0, "fragment UserCard on User { name email }"),
        (0, "query A { account(id: 1) { owner { name email id } } }"),
    ]);

    let extends: Vec<_> = analysis
        .overlaps
        .iter()
        .filter(|o| o.kind == OverlapKind::Extends)
        .collect();
    assert_eq!(extends.len(), 1, "{:#?}", analysis.overlaps);
    assert_eq!(extends[0].fragment, "UserCard");
    assert_eq!(extends[0].extra, vec!["id".to_string()]);
}

#[test]
fn two_fragments_with_the_same_body_are_one_finding() {
    let analysis = analyze(&[
        (0, "fragment UserCard on User { name email }"),
        (0, "fragment UserSummary on User { name email }"),
    ]);

    let matches: Vec<_> = analysis
        .overlaps
        .iter()
        .filter(|o| o.kind == OverlapKind::Matches)
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "a symmetric pair should be reported once: {:#?}",
        analysis.overlaps
    );
}

#[test]
fn a_fragment_does_not_duplicate_itself() {
    let analysis = analyze(&[(0, "fragment UserCard on User { name email }")]);
    assert!(analysis.overlaps.is_empty(), "{:#?}", analysis.overlaps);
}

#[test]
fn finds_a_group_shared_by_enough_definitions() {
    let analysis = analyze(&[
        (
            0,
            "query A { account(id: 1) { address { line1 city country } } }",
        ),
        (
            0,
            "query B { location(id: 1) { address { line1 city country } } }",
        ),
    ]);

    let group = analysis
        .groups
        .iter()
        .find(|g| g.type_name == "Address")
        .expect("expected an Address group");
    assert_eq!(group.members, vec!["city", "country", "line1"]);
    assert_eq!(group.definitions.len(), 2);
    assert_eq!(group.scope, Scope::InProject);
}

#[test]
fn finds_a_group_shared_only_partially() {
    // Neither selection set equals the other, so exact matching would miss this.
    let analysis = analyze(&[
        (
            0,
            "query A { account(id: 1) { address { line1 city country } } }",
        ),
        (
            0,
            "query B { location(id: 1) { address { line1 city postalCode } } }",
        ),
    ]);

    let group = analysis
        .groups
        .iter()
        .find(|g| g.type_name == "Address")
        .expect("expected an Address group");
    assert_eq!(group.members, vec!["city", "line1"]);
    assert!(group.sites.iter().all(|s| !s.exact));
}

#[test]
fn respects_min_uses() {
    let sources = [(
        0,
        "query A { account(id: 1) { address { line1 city country } } }",
    )];
    let analysis = analyze_with(
        Options {
            min_fields: 2,
            min_uses: 2,
            ..Default::default()
        },
        &sources,
    );
    assert!(analysis.groups.is_empty(), "{:#?}", analysis.groups);
}

#[test]
fn mandated_fields_do_not_count_toward_min_fields() {
    let mandated = mandated_for_project(0, &["id", "permissions"]);

    let sources = [
        (
            0,
            "query A { account(id: 1) { owner { id permissions name } } }",
        ),
        (
            0,
            "query B { account(id: 2) { owner { id permissions name } } }",
        ),
    ];

    // Two of the three fields are mandated, so only `name` counts.
    let analysis = analyze_with(
        Options {
            min_fields: 2,
            min_uses: 2,
            mandated_by_project: mandated.clone(),
        },
        &sources,
    );
    assert!(
        analysis.groups.iter().all(|g| g.type_name != "User"),
        "{:#?}",
        analysis.groups
    );

    // With the same fields counted normally, the group is reported.
    let analysis = analyze_with(
        Options {
            min_fields: 2,
            min_uses: 2,
            ..Default::default()
        },
        &sources,
    );
    assert!(analysis.groups.iter().any(|g| g.type_name == "User"));
}

#[test]
fn tags_a_group_that_spans_projects() {
    let analysis = analyze(&[
        (
            0,
            "query A { account(id: 1) { address { line1 city country } } }",
        ),
        (
            1,
            "query B { location(id: 1) { address { line1 city country } } }",
        ),
    ]);

    let group = analysis
        .groups
        .iter()
        .find(|g| g.type_name == "Address")
        .expect("expected an Address group");
    assert_eq!(group.scope, Scope::CrossProject);
}

#[test]
fn different_arguments_are_different_members() {
    let analysis = analyze(&[
        (
            0,
            "query A { playlist(id: 1) { name description trackCount(market: \"US\") } }",
        ),
        (
            0,
            "query B { playlist(id: 2) { name description trackCount(market: \"SE\") } }",
        ),
    ]);

    let group = analysis
        .groups
        .iter()
        .find(|g| g.type_name == "Playlist")
        .expect("expected a Playlist group");
    assert_eq!(
        group.members,
        vec!["description", "name"],
        "a field selected with different arguments is not the same member"
    );
}

#[test]
fn reports_only_maximal_groups() {
    // Every definition selects all three fields, so `{line1 city}` and
    // `{line1 country}` say nothing the full group does not.
    let analysis = analyze(&[
        (
            0,
            "query A { account(id: 1) { address { line1 city country } } }",
        ),
        (
            0,
            "query B { location(id: 1) { address { line1 city country } } }",
        ),
        (
            0,
            "query C { account(id: 3) { address { line1 city country } } }",
        ),
    ]);

    let address: Vec<_> = analysis
        .groups
        .iter()
        .filter(|g| g.type_name == "Address")
        .collect();
    assert_eq!(address.len(), 1, "{:#?}", address);
    assert_eq!(address[0].members.len(), 3);
}

#[test]
fn a_lone_spread_is_not_a_selection_worth_reporting() {
    let analysis = analyze(&[
        (0, "fragment UserCard on User { name email }"),
        (0, "query A { account(id: 1) { owner { ...UserCard } } }"),
        (0, "query B { account(id: 2) { owner { ...UserCard } } }"),
    ]);

    assert!(
        analysis.groups.iter().all(|g| g.type_name != "User"),
        "{:#?}",
        analysis.groups
    );
}

#[test]
fn a_group_an_existing_fragment_covers_is_marked() {
    let analysis = analyze(&[
        (0, "fragment UserCard on User { name email }"),
        (0, "query A { account(id: 1) { owner { name email } } }"),
        (0, "query B { account(id: 2) { owner { name email } } }"),
    ]);

    let group = analysis
        .groups
        .iter()
        .find(|g| g.type_name == "User")
        .expect("expected a User group");
    assert_eq!(group.covered_by.as_deref(), Some("UserCard"));
}

#[test]
fn new_fragment_skips_shapes_a_fragment_already_covers() {
    // The shape recurs, but `UserCard` is that shape, so the finding belongs to
    // matches_fragment rather than to "consider a fragment".
    let analysis = analyze(&[
        (0, "fragment UserCard on User { name email }"),
        (0, "query A { account(id: 1) { owner { name email } } }"),
        (0, "query B { account(id: 2) { owner { name email } } }"),
    ]);

    let mut rule = RepeatedSelectionsRule::new(RepeatedSelectionKind::NewFragment);
    rule.min_fields = 2;
    rule.min_uses = 2;

    let findings = repeated_selections::findings_for_rules(&analysis, &[rule]);
    assert!(findings.is_empty(), "{:#?}", findings);
}

#[test]
fn new_fragment_reports_one_finding_per_shape() {
    let analysis = analyze(&[
        (
            0,
            "query A { account(id: 1) { address { line1 city country } } }",
        ),
        (
            0,
            "query B { location(id: 1) { address { line1 city country } } }",
        ),
        (
            0,
            "query C { account(id: 2) { address { line1 city country } } }",
        ),
    ]);

    let mut rule = RepeatedSelectionsRule::new(RepeatedSelectionKind::NewFragment);
    rule.min_fields = 2;
    rule.min_uses = 2;

    let findings = repeated_selections::findings_for_rules(&analysis, &[rule]);
    assert_eq!(
        findings.len(),
        1,
        "one shape is one finding: {:#?}",
        findings
    );
    assert_eq!(findings[0].code, "new_fragment");
    assert!(findings[0].span.is_some());
    // The other places ride along rather than becoming their own findings.
    assert_eq!(findings[0].related.len(), 2, "{:#?}", findings[0]);
    assert!(findings[0].related.iter().all(|r| r.span.is_some()));
}

#[test]
fn a_shape_anchors_at_its_first_site_in_path_order() {
    let sources = [
        (
            0,
            "query B { location(id: 1) { address { line1 city country } } }",
        ),
        (
            0,
            "query A { account(id: 1) { address { line1 city country } } }",
        ),
    ];
    let analysis = analyze(&sources);

    let mut rule = RepeatedSelectionsRule::new(RepeatedSelectionKind::NewFragment);
    rule.min_fields = 2;
    rule.min_uses = 2;

    let findings = repeated_selections::findings_for_rules(&analysis, &[rule]);
    assert_eq!(findings.len(), 1);
    // doc0.graphql sorts before doc1.graphql, so the anchor is stable and does
    // not depend on which definition the walk reached first.
    assert!(
        findings[0].path.ends_with("doc0.graphql"),
        "{:#?}",
        findings[0]
    );
    assert_eq!(findings[0].related.len(), 1);
    assert!(findings[0].related[0].path.ends_with("doc1.graphql"));
}

#[test]
fn ignore_types_suppresses_a_type() {
    let analysis = analyze(&[
        (
            0,
            "query A { account(id: 1) { address { line1 city country } } }",
        ),
        (
            0,
            "query B { location(id: 1) { address { line1 city country } } }",
        ),
    ]);

    let mut rule = RepeatedSelectionsRule::new(RepeatedSelectionKind::NewFragment);
    rule.min_fields = 2;
    rule.min_uses = 2;
    rule.ignore_types = vec!["Address".to_string()];

    let findings = repeated_selections::findings_for_rules(&analysis, &[rule]);
    assert!(findings.is_empty(), "{:#?}", findings);
}

#[test]
fn each_entry_reports_at_its_own_severity() {
    let analysis = analyze(&[
        (0, "fragment UserCard on User { name email }"),
        (0, "query A { account(id: 1) { owner { name email } } }"),
        (0, "query B { account(id: 2) { owner { name email id } } }"),
    ]);

    let mut matches = RepeatedSelectionsRule::new(RepeatedSelectionKind::MatchesFragment);
    matches.min_fields = 2;
    matches.severity = Severity::Error;
    let mut extends = RepeatedSelectionsRule::new(RepeatedSelectionKind::ExtendsFragment);
    extends.min_fields = 2;
    extends.severity = Severity::Info;

    let findings = repeated_selections::findings_for_rules(&analysis, &[matches, extends]);
    let errors = findings.iter().filter(|f| f.severity == Severity::Error);
    let infos = findings.iter().filter(|f| f.severity == Severity::Info);
    assert_eq!(errors.count(), 1, "{:#?}", findings);
    assert_eq!(infos.count(), 1, "{:#?}", findings);
}

#[test]
fn min_fields_on_an_overlap_measures_the_fragment() {
    let analysis = analyze(&[
        (0, "fragment UserCard on User { name email }"),
        (0, "query A { account(id: 1) { owner { name email } } }"),
    ]);

    let mut rule = RepeatedSelectionsRule::new(RepeatedSelectionKind::MatchesFragment);
    rule.min_fields = 3;
    assert!(
        repeated_selections::findings_for_rules(&analysis, &[rule]).is_empty(),
        "a two-field fragment should not clear a three-field threshold"
    );

    let mut rule = RepeatedSelectionsRule::new(RepeatedSelectionKind::MatchesFragment);
    rule.min_fields = 2;
    assert_eq!(
        repeated_selections::findings_for_rules(&analysis, &[rule]).len(),
        1
    );
}

#[test]
fn a_mandated_field_does_not_count_even_when_aliased() {
    // Whether a member counts is decided during collection, from the Selection,
    // where the field name is available. Everything afterwards sees only the
    // serialized signature, so `userId: id` has to stay recognised as the
    // mandated `id` — matching signatures against configured field names would
    // silently start counting it.
    let mandated = mandated_for_project(0, &["id"]);

    let schema = schema();
    let docs = [
        DocumentSource {
            path: Path::new("doc0.graphql"),
            project_idx: 0,
            source: "fragment UserCard on User { userId: id name }",
        },
        DocumentSource {
            path: Path::new("doc1.graphql"),
            project_idx: 0,
            source: "query A { account(id: 1) { owner { userId: id name } } }",
        },
    ];

    // Collected permissively, the way `check` does when entries disagree on
    // min_fields, so the entry's own threshold is what decides.
    let analysis = repeated_selections::analyze(
        &schema,
        &docs,
        &Options {
            min_fields: 1,
            min_uses: 2,
            mandated_by_project: mandated,
        },
    );

    let overlap = analysis
        .overlaps
        .iter()
        .find(|o| o.kind == OverlapKind::Matches)
        .expect("expected the duplicate to be found");
    assert_eq!(
        overlap.shared, 1,
        "only `name` describes this fragment; `userId: id` is the mandated id"
    );

    let mut rule = RepeatedSelectionsRule::new(RepeatedSelectionKind::MatchesFragment);
    rule.min_fields = 2;
    assert!(
        repeated_selections::findings_for_rules(&analysis, &[rule]).is_empty(),
        "a fragment of one meaningful field should not clear a threshold of two"
    );
}

#[test]
fn a_field_one_project_mandates_and_another_chooses_still_counts() {
    // Four projects in a real workspace turn `required_fields` off. Where a
    // project does not impose `id`, whoever wrote the selection chose it, so a
    // group spanning both projects has to count it — otherwise the threshold is
    // stricter there than the configuration asks for.
    let mandated = mandated_for_project(0, &["id"]);

    let sources = [
        (0, "query A { account(id: 1) { id name } }"),
        (1, "query B { account(id: 2) { id name } }"),
    ];

    let analysis = analyze_with(
        Options {
            min_fields: 2,
            min_uses: 2,
            mandated_by_project: mandated,
        },
        &sources,
    );

    let group = analysis
        .groups
        .iter()
        .find(|g| g.type_name == "Account")
        .expect("expected an Account group");
    assert_eq!(group.members, vec!["id", "name"]);
    assert_eq!(
        group.counted, 2,
        "project 1 does not mandate `id`, so it describes the selection there"
    );
}

#[test]
fn a_field_every_selecting_project_mandates_does_not_count() {
    let mut mandated = mandated_for_project(0, &["id"]);
    mandated.insert(1, ["id".to_string()].into_iter().collect());

    let sources = [
        (0, "query A { account(id: 1) { id name } }"),
        (1, "query B { account(id: 2) { id name } }"),
    ];

    let analysis = analyze_with(
        Options {
            min_fields: 1,
            min_uses: 2,
            mandated_by_project: mandated,
        },
        &sources,
    );

    let group = analysis
        .groups
        .iter()
        .find(|g| g.type_name == "Account")
        .expect("expected an Account group");
    assert_eq!(group.members, vec!["id", "name"]);
    assert_eq!(group.counted, 1, "only `name` describes the selection");
}
