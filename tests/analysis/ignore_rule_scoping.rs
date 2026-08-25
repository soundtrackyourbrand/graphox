//! `# graphox-ignore` can name the rules it covers.
//!
//! One comment used to speak for every rule that consulted it, so silencing a
//! deprecation warning on a field also switched off the field rules on what
//! that field selects — with no way to have one without the other. A comment
//! may now name rules; a bare one still means all of them, because that is what
//! every comment already in a codebase means.

use ahash::AHashMap;
use graphox::Config;
use graphox::config::{ForbiddenFieldRule, RequiredFieldRule, RulesConfig};
use graphox::features::diagnostics::DocumentDiagnostics;

use crate::support::{create_doc, fixtures};

/// A deprecated union field whose members carry a required field: the shape
/// where the two rules collide over one comment.
fn document(comment: &str) -> String {
    format!(
        "query GetPlayback {{
  playback {{
    ...PlaybackInfo
  }}
}}
fragment PlaylistInfo on Playlist {{
  id
  name
}}
fragment PlaybackInfo on Playback {{
  id
  current {{
    id
    source {{{comment}
      ...PlaylistInfo
    }}
  }}
}}"
    )
}

fn counts(comment: &str) -> (usize, usize) {
    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "permissions".to_string(),
        RequiredFieldRule::new_operations(vec!["query".to_string()]),
    );
    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));
    let schema = fixtures::playable_source_schema()
        .clone()
        .validate()
        .unwrap();
    let text = document(comment);
    let doc = create_doc("file:///scoping.graphql", &text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);
    (
        diagnostics
            .iter()
            .filter(|d| d.message.contains("deprecated"))
            .count(),
        diagnostics
            .iter()
            .filter(|d| d.message.starts_with("Required"))
            .count(),
    )
}

#[test]
#[ntest::timeout(300)]
fn without_a_comment_both_rules_report() {
    assert_eq!(counts(""), (1, 1));
}

#[test]
#[ntest::timeout(300)]
fn a_bare_comment_still_covers_every_rule() {
    assert_eq!(counts(" # graphox-ignore"), (0, 0));
}

/// The case this was built for: keep the deprecation quiet, keep the field rule
/// working on the members the deprecated field selects.
#[test]
#[ntest::timeout(300)]
fn naming_one_rule_leaves_the_others_in_force() {
    assert_eq!(counts(" # graphox-ignore deprecated"), (0, 1));
    assert_eq!(counts(" # graphox-ignore required_fields"), (1, 0));
}

#[test]
#[ntest::timeout(300)]
fn several_rules_can_be_named() {
    assert_eq!(
        counts(" # graphox-ignore deprecated, required_fields"),
        (0, 0)
    );
    assert_eq!(
        counts(" # graphox-ignore deprecated required_fields"),
        (0, 0)
    );
}

/// A comment that names no rule is bare, however much text follows it. Comments
/// carrying an explanation predate scoping and have to keep working, or adding
/// this feature would quietly switch rules back on across a codebase.
#[test]
#[ntest::timeout(300)]
fn prose_after_the_marker_is_still_a_bare_comment() {
    assert_eq!(counts(" # graphox-ignore legacy zones, see PLAT-1"), (0, 0));
    assert_eq!(counts(" # graphox-ignore -- do not remove"), (0, 0));
}

/// A misspelled rule name names no rule, so it reads as bare. That
/// over-suppresses rather than under-suppresses, which is the safer of the two
/// failures, and the name is recoverable for reporting rather than lost.
#[test]
#[ntest::timeout(300)]
fn a_misspelled_rule_name_reads_as_bare() {
    assert_eq!(counts(" # graphox-ignore deprecatd"), (0, 0));
    assert_eq!(
        graphox::document::unrecognised_ignore_rule_names(" # graphox-ignore deprecatd"),
        vec!["deprecatd".to_string()],
    );
    assert!(
        graphox::document::unrecognised_ignore_rule_names(" # graphox-ignore legacy zones")
            .is_empty(),
        "prose is not a misspelled rule name"
    );
    assert!(
        graphox::document::unrecognised_ignore_rule_names(" # graphox-ignore required_fields")
            .is_empty(),
    );
}

/// Scoping reaches the placements suppression already worked at, not just the
/// field it was built for.
#[test]
#[ntest::timeout(300)]
fn scoping_applies_at_a_spread_as_well() {
    let text = "subscription S {
  zoneUpdate {
    source {
      ...M # graphox-ignore required_fields
    }
  }
}
fragment M on Manual {
  id
  secret
}";
    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "secret".to_string(),
        ForbiddenFieldRule::new_operations(vec!["subscription".to_string()]),
    );
    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));
    let schema = fixtures::syntax_matrix_schema().clone().validate().unwrap();
    let doc = create_doc("file:///scoping.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    // The comment names a different rule, so the forbidden finding stands.
    assert_eq!(
        diagnostics.len(),
        1,
        "expected the forbidden finding to survive a required_fields-only ignore: {diagnostics:?}"
    );
}
