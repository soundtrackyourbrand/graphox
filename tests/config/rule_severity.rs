use graphox::Config;
use graphox::config::Severity;
use std::fs;
use tempfile::TempDir;

/// Writes a minimal valid config with the given `rules:` block and loads it.
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
fn boolean_rule_keeps_its_default_severity() {
    let config = load_with_rules(
        "rules:\n  no_duplicate_fields: true\n  no_unused_fragments: true\n  unique_operation_name: true\n",
    );
    let rules = config.rules();

    assert!(rules.no_duplicate_fields());
    assert!(rules.no_unused_fragments());
    assert!(rules.unique_operation_name());

    // Enabling a rule without naming a severity must report what it always has.
    assert_eq!(rules.no_duplicate_fields_severity(), Severity::Error);
    assert_eq!(rules.unique_operation_name_severity(), Severity::Error);
    assert_eq!(rules.no_unused_fragments_severity(), Severity::Warning);
}

#[test]
#[ntest::timeout(3000)]
fn unset_rule_still_reports_its_default_severity() {
    let config = load_with_rules("");
    let rules = config.rules();

    assert!(!rules.no_duplicate_fields());
    assert_eq!(rules.no_duplicate_fields_severity(), Severity::Error);
    assert_eq!(rules.no_unused_fragments_severity(), Severity::Warning);
}

#[test]
#[ntest::timeout(3000)]
fn bare_severity_enables_the_rule() {
    let config = load_with_rules("rules:\n  no_duplicate_fields: warning\n");
    let rules = config.rules();

    assert!(rules.no_duplicate_fields());
    assert_eq!(rules.no_duplicate_fields_severity(), Severity::Warning);
}

#[test]
#[ntest::timeout(3000)]
fn long_form_sets_enabled_and_severity() {
    let config = load_with_rules(
        "rules:\n  no_duplicate_fields:\n    enabled: true\n    severity: info\n  no_unused_fragments:\n    enabled: false\n    severity: error\n",
    );
    let rules = config.rules();

    assert!(rules.no_duplicate_fields());
    assert_eq!(rules.no_duplicate_fields_severity(), Severity::Info);

    // A severity on a disabled rule is inert but still parsed.
    assert!(!rules.no_unused_fragments());
    assert_eq!(rules.no_unused_fragments_severity(), Severity::Error);
}

#[test]
#[ntest::timeout(3000)]
fn long_form_without_enabled_is_an_opt_in() {
    let config = load_with_rules("rules:\n  no_duplicate_fields:\n    severity: warning\n");
    let rules = config.rules();

    assert!(rules.no_duplicate_fields());
    assert_eq!(rules.no_duplicate_fields_severity(), Severity::Warning);
}

#[test]
#[ntest::timeout(3000)]
fn unknown_severity_falls_back_to_the_default() {
    let config = load_with_rules(
        "rules:\n  no_duplicate_fields:\n    enabled: true\n    severity: nonsense\n",
    );
    let rules = config.rules();

    assert!(rules.no_duplicate_fields());
    assert_eq!(rules.no_duplicate_fields_severity(), Severity::Error);
}

#[test]
#[ntest::timeout(3000)]
fn an_unknown_bare_severity_still_enables_the_rule() {
    // The short and long forms have to agree: naming a severity is an opt-in
    // either way, and the warning says the rule's default is used.
    let config = load_with_rules("rules:\n  no_duplicate_fields: nonsense\n");
    let rules = config.rules();

    assert!(rules.no_duplicate_fields());
    assert_eq!(rules.no_duplicate_fields_severity(), Severity::Error);
}

#[test]
#[ntest::timeout(3000)]
fn field_rules_carry_a_severity() {
    let config = load_with_rules(
        "rules:\n  required_fields:\n    id: true\n    permissions:\n      enabled: [query]\n      severity: warning\n      reason: still rolling out\n",
    );
    let rules = config.rules();

    let id = rules.get_required_rule("Query", "id").unwrap();
    assert_eq!(id.severity(), Severity::Error);

    let permissions = rules.get_required_rule("Query", "permissions").unwrap();
    assert_eq!(permissions.severity(), Severity::Warning);
    assert_eq!(permissions.reason(), Some("still rolling out"));
    assert!(permissions.applies_to_operation("query"));
}

#[test]
#[ntest::timeout(3000)]
fn project_rules_override_severity() {
    let temp_dir = TempDir::new().unwrap();
    fs::write(
        temp_dir.path().join("schema.graphql"),
        "type Query { user: String }",
    )
    .unwrap();
    fs::write(
        temp_dir.path().join("graphox.yaml"),
        "rules:\n  no_duplicate_fields: error\nprojects:\n  - name: test\n    schema: schema.graphql\n    include: \"*.graphql\"\n    rules:\n      no_duplicate_fields: warning\n",
    )
    .unwrap();

    let config = Config::load_from_dir(temp_dir.path()).unwrap().unwrap();
    let project = &config.projects()[0];
    let effective = config.rules().merge(project.rules().unwrap());

    assert!(effective.no_duplicate_fields());
    assert_eq!(effective.no_duplicate_fields_severity(), Severity::Warning);
}

#[test]
fn fail_on_matches_at_or_above_its_level() {
    use graphox::config::Severity as S;
    let error = Some(S::Error.as_lsp());
    let warning = Some(S::Warning.as_lsp());
    let info = Some(S::Info.as_lsp());

    assert!(S::Error.is_met_by(error));
    assert!(!S::Error.is_met_by(warning));
    assert!(!S::Error.is_met_by(info));

    assert!(S::Warning.is_met_by(error));
    assert!(S::Warning.is_met_by(warning));
    assert!(!S::Warning.is_met_by(info));

    assert!(S::Info.is_met_by(error));
    assert!(S::Info.is_met_by(warning));
    assert!(S::Info.is_met_by(info));

    // A diagnostic with no severity has never counted as a failure.
    assert!(!S::Info.is_met_by(None));
}
