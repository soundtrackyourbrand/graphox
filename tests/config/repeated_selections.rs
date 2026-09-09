use graphox::Config;
use graphox::config::{RepeatedSelectionKind, Severity};
use std::fs;
use tempfile::TempDir;

fn load_with_rules(rules: &str) -> Config {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("schema.graphql"),
        "type Query { user: String }",
    )
    .unwrap();
    let config_content = format!(
        "projects:\n  - name: test\n    schema: schema.graphql\n    include: \"*.graphql\"\n{}",
        rules
    );
    fs::write(temp_dir.path().join("graphox.yaml"), config_content).unwrap();
    Config::load_from_dir(temp_dir.path()).unwrap().unwrap()
}

#[test]
#[ntest::timeout(3000)]
fn unset_by_default() {
    let config = load_with_rules("");
    let rules = config.rules();
    assert!(rules.repeated_selections().is_empty());
}

#[test]
#[ntest::timeout(3000)]
fn each_kind_has_its_own_defaults() {
    let config = load_with_rules(
        "rules:\n  repeated_selections:\n    - kind: matches_fragment\n    - kind: extends_fragment\n    - kind: new_fragment\n",
    );
    let config_rules = config.rules();
    let rules = config_rules.repeated_selections();
    assert_eq!(rules.len(), 3);

    assert_eq!(rules[0].kind, RepeatedSelectionKind::MatchesFragment);
    assert_eq!(rules[0].severity, Severity::Error);
    assert_eq!(rules[0].min_fields, 3);
    // A single re-inlined copy is already a finding.
    assert_eq!(rules[0].min_uses, 1);

    assert_eq!(rules[1].kind, RepeatedSelectionKind::ExtendsFragment);
    assert_eq!(rules[1].severity, Severity::Warning);
    assert_eq!(rules[1].min_fields, 4);
    assert_eq!(rules[1].min_uses, 1);

    assert_eq!(rules[2].kind, RepeatedSelectionKind::NewFragment);
    assert_eq!(rules[2].severity, Severity::Info);
    assert_eq!(rules[2].min_fields, 6);
    assert_eq!(rules[2].min_uses, 4);
}

#[test]
#[ntest::timeout(3000)]
fn entries_are_configured_independently() {
    let config = load_with_rules(
        "rules:\n  repeated_selections:\n    - kind: matches_fragment\n      min_fields: 1\n      severity: error\n    - kind: new_fragment\n      min_fields: 6\n      min_uses: 8\n      severity: info\n      ignore_types: [PageInfo, Address]\n",
    );
    let config_rules = config.rules();
    let rules = config_rules.repeated_selections();

    assert_eq!(rules[0].min_fields, 1);
    assert_eq!(rules[0].severity, Severity::Error);

    assert_eq!(rules[1].min_fields, 6);
    assert_eq!(rules[1].min_uses, 8);
    assert_eq!(rules[1].severity, Severity::Info);
    assert!(rules[1].ignores("PageInfo"));
    assert!(rules[1].ignores("Address"));
    assert!(!rules[1].ignores("Account"));
}

#[test]
#[ntest::timeout(3000)]
fn min_uses_is_rejected_on_the_fragment_kinds() {
    let config = load_with_rules(
        "rules:\n  repeated_selections:\n    - kind: matches_fragment\n      min_uses: 9\n",
    );
    let config_rules = config.rules();
    let rules = config_rules.repeated_selections();
    // The entry survives; only the key that cannot apply is dropped.
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].min_uses, 1);
}

#[test]
#[ntest::timeout(3000)]
fn an_unknown_kind_drops_only_that_entry() {
    let config = load_with_rules(
        "rules:\n  repeated_selections:\n    - kind: nonsense\n    - kind: new_fragment\n",
    );
    let config_rules = config.rules();
    let rules = config_rules.repeated_selections();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].kind, RepeatedSelectionKind::NewFragment);
}

#[test]
#[ntest::timeout(3000)]
fn project_rules_replace_the_list() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("schema.graphql"),
        "type Query { user: String }",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("graphox.yaml"),
        "rules:\n  repeated_selections:\n    - kind: matches_fragment\nprojects:\n  - name: test\n    schema: schema.graphql\n    include: \"*.graphql\"\n    rules:\n      repeated_selections:\n        - kind: new_fragment\n          min_uses: 5\n",
    )
    .unwrap();

    let config = Config::load_from_dir(temp_dir.path()).unwrap().unwrap();
    let global = config.rules();
    let effective = global.merge(config.projects()[0].rules().unwrap());
    let rules = effective.repeated_selections();

    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].kind, RepeatedSelectionKind::NewFragment);
    assert_eq!(rules[0].min_uses, 5);
}
