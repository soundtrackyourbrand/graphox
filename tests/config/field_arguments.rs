use graphox::config::{
    ArgumentLimit, FieldArgumentsRule, FieldCoordinate, LimitValue, RulesConfig, Severity,
};

fn rules(yaml: &str) -> RulesConfig {
    let docs = yaml_rust2::YamlLoader::load_from_str(yaml).unwrap();
    RulesConfig::from_yaml(&docs[0]).unwrap()
}

fn rule<'a>(rules: &'a RulesConfig, coordinate: &str) -> Option<&'a FieldArgumentsRule> {
    rules
        .field_arguments()
        .get(&FieldCoordinate::parse(coordinate).unwrap())
}

#[test]
fn reads_a_maximum_a_list_of_values_a_reason_and_a_severity() {
    let rules = rules(
        r#"
        field_arguments:
          Playlist.tracks:
            first: { max: 100 }
            reason: page through it instead
          Album.tracks:
            first: [100, 500]
            order: [ASC]
            severity: warning
        "#,
    );

    let playlist = rule(&rules, "Playlist.tracks").unwrap();
    assert_eq!(playlist.limit("first"), Some(&ArgumentLimit::Max(100)));
    assert_eq!(playlist.reason.as_deref(), Some("page through it instead"));
    assert_eq!(playlist.severity(), Severity::Error);

    let album = rule(&rules, "Album.tracks").unwrap();
    assert_eq!(
        album.limit("first"),
        Some(&ArgumentLimit::Values(vec![
            LimitValue::Int(100),
            LimitValue::Int(500)
        ]))
    );
    assert_eq!(
        album.limit("order"),
        Some(&ArgumentLimit::Values(vec![LimitValue::String(
            "ASC".to_string()
        )]))
    );
    assert_eq!(album.severity(), Severity::Warning);
}

#[test]
fn an_argument_named_reason_is_told_apart_by_its_value() {
    let rules = rules(
        r#"
        field_arguments:
          Report.entries:
            reason: { max: 3 }
        "#,
    );

    let report = rule(&rules, "Report.entries").unwrap();
    assert_eq!(report.limit("reason"), Some(&ArgumentLimit::Max(3)));
    assert_eq!(report.reason, None);
}

#[test]
fn malformed_entries_are_skipped() {
    let rules = rules(
        r#"
        field_arguments:
          tracks: { first: { max: 1 } }
          Playlist. tracks: { first: { max: 1 } }
          Album.tracks:
            first: 100
          Track.lyrics:
            reason: nothing is limited here
          Playlist.items:
            first: { max: 50 }
            last: "lots"
        "#,
    );

    let coordinates: Vec<String> = rules
        .field_arguments()
        .keys()
        .map(ToString::to_string)
        .collect();
    assert_eq!(coordinates, vec!["Playlist.items".to_string()]);
    let items = rule(&rules, "Playlist.items").unwrap();
    assert_eq!(items.arguments.len(), 1, "the malformed `last` is dropped");
}

#[test]
fn a_projects_limits_replace_the_top_level_ones() {
    let top = rules("field_arguments:\n  Playlist.tracks:\n    first: { max: 100 }\n");
    let project = rules("field_arguments:\n  Album.tracks:\n    first: { max: 10 }\n");

    let merged = top.merge(&project);
    assert!(rule(&merged, "Playlist.tracks").is_none());
    assert!(rule(&merged, "Album.tracks").is_some());

    let merged = top.merge(&rules("no_duplicate_fields: true\n"));
    assert!(rule(&merged, "Playlist.tracks").is_some());
}
