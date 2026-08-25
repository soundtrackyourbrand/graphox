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
            .filter(|d| d.message.contains("is deprecated"))
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
    assert_eq!(
        counts(" # graphox-ignore: legacy zones, see PLAT-1"),
        (0, 0)
    );
    assert_eq!(counts(" # graphox-ignore -- do not remove"), (0, 0));
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

/// Everything before the explanation marker is a rule list, so a word there
/// that names no rule is either a misspelling or prose missing its marker.
/// Both warn, because from the outside nothing looks wrong while the narrowing
/// its author wrote never happened.
fn warnings(comment: &str) -> Vec<String> {
    let schema = fixtures::playable_source_schema()
        .clone()
        .validate()
        .unwrap();
    let text = document(comment);
    let doc = create_doc("file:///scoping.graphql", &text);
    doc.get_semantic_diagnostics(&schema, &[], None, None, false, true)
        .iter()
        .filter(|d| {
            matches!(
                &d.code,
                Some(tower_lsp_server::ls_types::NumberOrString::String(c))
                    if c == "unknown_ignore_rule"
            )
        })
        .map(|d| d.message.clone())
        .collect()
}

#[test]
#[ntest::timeout(300)]
fn an_unknown_rule_name_warns() {
    let w = warnings(" # graphox-ignore deprecatd");
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("'deprecatd'"), "{w:?}");
    assert!(
        w[0].contains("deprecated"),
        "the message should list the real names: {w:?}"
    );
}

#[test]
#[ntest::timeout(300)]
fn prose_without_a_marker_warns() {
    let w = warnings(" # graphox-ignore legacy zones");
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("'legacy'"), "{w:?}");
}

#[test]
#[ntest::timeout(300)]
fn an_explanation_after_a_marker_is_quiet() {
    for comment in [
        " # graphox-ignore: legacy zones, see PLAT-1",
        " # graphox-ignore - legacy zones",
        " # graphox-ignore (marked as test in the public API)",
        " # graphox-ignore -- do not remove",
        " # graphox-ignore deprecated: broken upstream",
        " # graphox-ignore deprecated, required_fields (both, for now)",
    ] {
        assert!(
            warnings(comment).is_empty(),
            "unexpected warning for {comment:?}"
        );
    }
}

#[test]
#[ntest::timeout(300)]
fn a_bare_comment_and_a_named_rule_are_quiet() {
    assert!(warnings(" # graphox-ignore").is_empty());
    assert!(warnings(" # graphox-ignore deprecated").is_empty());
    assert!(warnings(" # graphox-ignore deprecated, required_fields").is_empty());
}

/// An explanation does not change what the comment covers.
#[test]
#[ntest::timeout(300)]
fn an_explanation_does_not_change_the_scope() {
    assert_eq!(counts(" # graphox-ignore: legacy zones"), (0, 0));
    assert_eq!(counts(" # graphox-ignore (legacy zones)"), (0, 0));
    assert_eq!(
        counts(" # graphox-ignore deprecated: broken upstream"),
        (0, 1)
    );
    assert_eq!(
        counts(" # graphox-ignore deprecated - broken upstream"),
        (0, 1)
    );
}

/// A comment naming nothing graphox knows still covers everything, so adding
/// the warning cannot make a rule start firing on its own.
#[test]
#[ntest::timeout(300)]
fn an_unknown_name_still_suppresses_everything() {
    assert_eq!(counts(" # graphox-ignore deprecatd"), (0, 0));
    assert_eq!(counts(" # graphox-ignore legacy zones"), (0, 0));
}

/// The warning has to describe what the comment actually does. A list of
/// nothing but unknown words falls back to covering everything; one unknown
/// word beside a correctly spelled rule narrows to that rule, which is the
/// opposite mistake and needs saying so.
#[test]
#[ntest::timeout(300)]
fn the_warning_states_the_real_scope() {
    let all = warnings(" # graphox-ignore legacy zones");
    assert_eq!(all.len(), 1, "{all:?}");
    assert!(
        all[0].contains("covers every rule"),
        "an all-unknown list covers everything: {all:?}"
    );

    let mixed = warnings(" # graphox-ignore required_fields we need this");
    assert_eq!(mixed.len(), 1, "{mixed:?}");
    assert!(
        mixed[0].contains("covers only required_fields"),
        "a known name beside an unknown word narrows: {mixed:?}"
    );
    // And the narrowing really did happen, so the message is not lying.
    assert_eq!(
        counts(" # graphox-ignore required_fields we need this"),
        (1, 0)
    );
}

/// A rule name written where the explanation goes narrows nothing, and the
/// comment silently covers everything. Both spellings are plausible enough to
/// be worth catching.
#[test]
#[ntest::timeout(300)]
fn a_rule_name_in_the_explanation_warns() {
    for comment in [
        " # graphox-ignore: deprecated",
        " # graphox-ignore (deprecated)",
        " # graphox-ignore - required_fields",
    ] {
        let w = warnings(comment);
        assert_eq!(w.len(), 1, "{comment:?} -> {w:?}");
        assert!(w[0].contains("narrows nothing"), "{comment:?} -> {w:?}");
        // It still suppresses everything, so nothing starts firing unasked.
        assert_eq!(counts(comment), (0, 0), "{comment:?}");
    }
}

/// An explanation that merely mentions a rule name later on is not the same
/// mistake and must stay quiet.
#[test]
#[ntest::timeout(300)]
fn an_explanation_mentioning_a_rule_later_is_quiet() {
    assert!(warnings(" # graphox-ignore: we cannot satisfy required_fields yet").is_empty());
    assert!(warnings(" # graphox-ignore deprecated: also required_fields, later").is_empty());
}
