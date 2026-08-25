//! Every syntactic route a selection can take to a field, checked against the
//! required/forbidden field rules.
//!
//! These rules work off response-key bookkeeping built while walking the
//! document, and their history is a series of shapes that bookkeeping could not
//! reach. A shape it cannot reach reports nothing at all, so the failure is
//! silent: `forbidden_fields` is the probe here precisely because a miss shows
//! up as zero diagnostics rather than a wrong message.

use ahash::AHashMap;
use graphox::Config;
use graphox::config::{ForbiddenFieldRule, RequiredFieldRule, RulesConfig};
use graphox::features::diagnostics::DocumentDiagnostics;
use tower_lsp_server::ls_types::Diagnostic;

use crate::support::{create_doc, fixtures};

fn forbidden_secret_in_subscriptions() -> Config {
    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "secret".to_string(),
        ForbiddenFieldRule::new_operations(vec!["subscription".to_string()]),
    );
    Config::default().with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields))
}

/// Diagnostics that are not about a forbidden field mean the case document is
/// itself wrong — a typo'd field, an impossible type condition — and the case
/// then proves nothing either way. Fail loudly rather than counting it.
fn split_diagnostics(diagnostics: &[Diagnostic]) -> (usize, Vec<String>) {
    let forbidden = diagnostics
        .iter()
        .filter(|d| d.message.contains("forbidden"))
        .count();
    let noise = diagnostics
        .iter()
        .filter(|d| !d.message.contains("forbidden"))
        .map(|d| d.message.clone())
        .collect();
    (forbidden, noise)
}

/// Documents whose subscription reaches `secret` by some route. Each selects it
/// exactly once, so each must report exactly once: too few means the shape is
/// invisible, too many means one selection is reported by two routes.
const REACHES_SECRET: &[(&str, &str)] = &[
    // ---- plain nesting ----
    ("op/field", r#"subscription S { zoneUpdate { secret } }"#),
    (
        "op/nested-field",
        r#"subscription S { zoneUpdate { meta { secret } } }"#,
    ),
    (
        "frag/root-field",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { secret }"#,
    ),
    (
        "frag/nested-field",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { meta { secret } }"#,
    ),
    // ---- type conditions in an operation body ----
    (
        "op/inline-union-leaf",
        r#"subscription S { zoneUpdate { source { ... on Manual { secret } } } }"#,
    ),
    (
        "op/inline-union-then-field",
        r#"subscription S { zoneUpdate { source { ... on ScheduleSource { schedule { secret } } } } }"#,
    ),
    // ---- type conditions in a fragment body ----
    (
        "frag/root-inline-leaf",
        r#"subscription S { zoneUpdate { source { ...F } } }
           fragment F on PlayableSource { ... on Manual { secret } }"#,
    ),
    (
        "frag/root-inline-then-field",
        r#"subscription S { zoneUpdate { source { ...F } } }
           fragment F on PlayableSource { ... on ScheduleSource { schedule { secret } } }"#,
    ),
    (
        "frag/nested-inline-union",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { source { ... on ScheduleSource { schedule { secret } } } }"#,
    ),
    (
        "frag/nested-inline-interface",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { item { ... on OtherItem { secret } } }"#,
    ),
    // ---- a spread as the leaf under a type condition ----
    (
        "frag/root-inline-spread-leaf",
        r#"subscription S { zoneUpdate { source { ...F } } }
           fragment F on PlayableSource { ... on Manual { ...M } }
           fragment M on Manual { secret }"#,
    ),
    (
        "frag/root-inline-spread-nesting",
        r#"subscription S { zoneUpdate { source { ...F } } }
           fragment F on PlayableSource { ... on ScheduleSource { ...SS } }
           fragment SS on ScheduleSource { schedule { secret } }"#,
    ),
    (
        "frag/nested-inline-spread-leaf",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { source { ... on Manual { ...M } } }
           fragment M on Manual { secret }"#,
    ),
    (
        "frag/nested-inline-spread-nesting",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { source { ... on ScheduleSource { ...SS } } }
           fragment SS on ScheduleSource { schedule { secret } }"#,
    ),
    // ---- inline fragments with no type condition, which select on the
    // ---- enclosing type and so are transparent to the response key ----
    (
        "op/no-tc",
        r#"subscription S { zoneUpdate { meta { ... { secret } } } }"#,
    ),
    (
        "op/no-tc-at-root-selection",
        r#"subscription S { zoneUpdate { ... { secret } } }"#,
    ),
    (
        "op/no-tc-stacked",
        r#"subscription S { zoneUpdate { meta { ... { ... { secret } } } } }"#,
    ),
    (
        "op/no-tc-with-directive",
        r#"subscription S($x: Boolean!) { zoneUpdate { meta { ... @include(if: $x) { secret } } } }"#,
    ),
    (
        "op/typed-then-no-tc",
        r#"subscription S { zoneUpdate { meta { ... on Meta { ... { secret } } } } }"#,
    ),
    (
        "op/no-tc-then-typed",
        r#"subscription S { zoneUpdate { meta { ... { ... on Meta { secret } } } } }"#,
    ),
    (
        "op/no-tc-under-union-member",
        r#"subscription S { zoneUpdate { source { ... on Manual { ... { secret } } } } }"#,
    ),
    (
        "frag/nested-no-tc",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { meta { ... { secret } } }"#,
    ),
    (
        "frag/root-no-tc",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { ... { meta { secret } } }"#,
    ),
    (
        "frag/no-tc-holding-spread",
        r#"subscription S { zoneUpdate { meta { ... { ...M } } } }
           fragment M on Meta { secret }"#,
    ),
    // ---- a spread whose own type condition narrows an abstract type, with
    // ---- no `... on X` written anywhere ----
    (
        "frag/spread-narrows-union-member",
        r#"subscription S { zoneUpdate { source { ...M } } }
           fragment M on Manual { id secret }"#,
    ),
    (
        "frag/spread-narrows-through-abstract-fragment",
        r#"subscription S { zoneUpdate { source { ...F } } }
           fragment F on PlayableSource { __typename ...M }
           fragment M on Manual { id secret }"#,
    ),
    (
        "frag/spread-narrows-interface-member",
        r#"subscription S { zoneUpdate { item { ...OI } } }
           fragment OI on OtherItem { id secret }"#,
    ),
    (
        "frag/spread-narrows-nested-in-fragment",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { source { ...M } }
           fragment M on Manual { id secret }"#,
    ),
    // ---- aliases, where the response-key path and the schema path diverge ----
    (
        "alias/on-union-field",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { s: source { ... on ScheduleSource { schedule { secret } } } }"#,
    ),
    (
        "alias/under-type-condition",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { source { ... on ScheduleSource { sch: schedule { secret } } } }"#,
    ),
    (
        "alias/holding-spread",
        r#"subscription S { zoneUpdate { m: meta { ...F } } }
           fragment F on Meta { schedule { secret } }"#,
    ),
    (
        "alias/on-the-forbidden-field",
        r#"subscription S { zoneUpdate { meta { safe: secret } } }"#,
    ),
    // ---- deeper chains through abstract types ----
    (
        "frag/union-member-union",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { source { ... on ScheduleSource { alt { ... on Manual { secret } } } } }"#,
    ),
    (
        "frag/union-member-interface",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { source { ... on ScheduleSource { inner { ... on OtherItem { secret } } } } }"#,
    ),
    (
        "frag/deep-below-type-condition",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { source { ... on ScheduleSource { schedule { deep { secret } } } } }"#,
    ),
    // ---- interface hierarchies ----
    (
        "frag/interface-implementing-interface",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { node { ... on OtherItem { secret } } }"#,
    ),
    (
        "frag/narrow-interface-to-interface-to-object",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { node { ... on Item { ... on OtherItem { secret } } } }"#,
    ),
    (
        "frag/redundant-nested-type-condition",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { source { ... on ScheduleSource { ... on ScheduleSource { schedule { secret } } } } }"#,
    ),
    // ---- lists ----
    (
        "frag/list-of-union",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { sources { ... on ScheduleSource { schedule { secret } } } }"#,
    ),
    // ---- chained spreads ----
    (
        "frag/spread-chain-through-type-condition",
        r#"subscription S { zoneUpdate { ...A } }
           fragment A on Zone { source { ...B } }
           fragment B on PlayableSource { ... on ScheduleSource { schedule { ...C } } }
           fragment C on Schedule { secret }"#,
    ),
    (
        "frag/top-level-spread-then-nesting",
        r#"subscription S { zoneUpdate { ...A } }
           fragment A on Zone { ...B }
           fragment B on Zone { source { ... on ScheduleSource { schedule { secret } } } }"#,
    ),
    (
        "op/inline-then-spread-that-nests",
        r#"subscription S { zoneUpdate { source { ... on ScheduleSource { ...SS } } } }
           fragment SS on ScheduleSource { schedule { secret } }"#,
    ),
    (
        "op/inline-on-object-then-spread",
        r#"subscription S { zoneUpdate { ... on Zone { ...F } } }
           fragment F on Zone { source { ... on ScheduleSource { schedule { secret } } } }"#,
    ),
    // ---- recursion, and siblings that must not mask the finding ----
    (
        "frag/below-recursive-field",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { child { source { ... on ScheduleSource { schedule { secret } } } } }"#,
    ),
    (
        "frag/type-condition-with-typename-sibling",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { source { __typename ... on ScheduleSource { schedule { id secret } } } }"#,
    ),
    (
        "frag/second-type-condition-sibling",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { source { ... on Manual { id } ... on ScheduleSource { schedule { secret } } } }"#,
    ),
];

#[test]
#[ntest::timeout(5000)]
fn forbidden_field_is_found_through_every_shape() {
    let config = forbidden_secret_in_subscriptions();
    let schema = fixtures::syntax_matrix_schema().clone().validate().unwrap();

    let mut misses = Vec::new();
    let mut extras = Vec::new();
    let mut invalid = Vec::new();

    for (label, text) in REACHES_SECRET {
        let doc = create_doc("file:///matrix.graphql", text);
        let diagnostics =
            doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);
        let (found, noise) = split_diagnostics(&diagnostics);

        if !noise.is_empty() {
            invalid.push(format!("  {label}: {noise:?}"));
        }
        match found {
            0 => misses.push(*label),
            1 => {}
            n => extras.push(format!("  {label}: {n}")),
        }
    }

    assert!(
        invalid.is_empty(),
        "cases produced unrelated diagnostics, so the documents are wrong:\n{}",
        invalid.join("\n")
    );
    assert!(
        misses.is_empty(),
        "{} of {} shapes reported nothing, so the rule cannot see them:\n  {}",
        misses.len(),
        REACHES_SECRET.len(),
        misses.join("\n  ")
    );
    assert!(
        extras.is_empty(),
        "shapes reported one selection more than once:\n{}",
        extras.join("\n")
    );
}

/// The mirror of the list above: documents that must stay silent. A rule that
/// fires here is worse than one that misses, because it cannot be worked around.
const REACHES_NOTHING: &[(&str, &str)] = &[
    (
        "silent/no-forbidden-field",
        r#"subscription S { zoneUpdate { meta { id schedule { id name } } } }"#,
    ),
    (
        "silent/type-condition-without-it",
        r#"subscription S { zoneUpdate { source { ... on ScheduleSource { schedule { id name } } } } }"#,
    ),
    (
        "silent/wrong-operation-type",
        r#"query Q { zone { meta { secret } } }"#,
    ),
    (
        "silent/wrong-operation-type-no-tc",
        r#"query Q { zone { meta { ... { secret } } } }"#,
    ),
    // The rule is about the field, not the response key it lands under.
    (
        "silent/benign-field-aliased-to-the-name",
        r#"subscription S { zoneUpdate { meta { secret: id } } }"#,
    ),
    (
        "silent/benign-alias-under-type-condition",
        r#"subscription S { zoneUpdate { source { ... on Manual { secret: id } } } }"#,
    ),
    (
        "silent/member-fragment-without-the-field",
        r#"subscription S { zoneUpdate { source { ...M } } }
           fragment M on Manual { id }"#,
    ),
    (
        "silent/fragment-supplying-nothing-relevant",
        r#"subscription S { zoneUpdate { source { ...F } } }
           fragment F on PlayableSource { __typename ... on Manual { id } }"#,
    ),
];

#[test]
#[ntest::timeout(5000)]
fn forbidden_field_stays_silent_where_it_should() {
    let config = forbidden_secret_in_subscriptions();
    let schema = fixtures::syntax_matrix_schema().clone().validate().unwrap();

    let mut spurious = Vec::new();
    for (label, text) in REACHES_NOTHING {
        let doc = create_doc("file:///matrix.graphql", text);
        let diagnostics =
            doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);
        if !diagnostics.is_empty() {
            let msgs: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
            spurious.push(format!("  {label}: {msgs:?}"));
        }
    }
    assert!(
        spurious.is_empty(),
        "shapes reported a diagnostic they should not:\n{}",
        spurious.join("\n")
    );
}

/// Where a `forbidden_fields` finding can be silenced.
///
/// The field is *there*, so it is annotated on itself — the narrowest placement,
/// and the line the diagnostic points at. A spread is the one exception: it is a
/// leaf in this document, there is nothing inside it to annotate, and silencing
/// at the spread covers this operation rather than every operation that spreads
/// the fragment.
///
/// Written with each construct on its own line. A comment after `source {` is
/// read as attached to that field, so a single-line document silently tests the
/// parent placement no matter where the comment appears to sit — that mistake
/// made two of these pass for the wrong reason once already.
const SUPPRESSED: &[(&str, &str)] = &[
    (
        "ignore/on-the-field-in-an-operation",
        "subscription S {\n zoneUpdate {\n meta {\n secret # graphox-ignore\n }\n }\n}",
    ),
    (
        "ignore/on-the-field-under-a-type-condition",
        "subscription S {\n zoneUpdate {\n source {\n ... on Manual {\n secret # graphox-ignore\n }\n }\n }\n}",
    ),
    (
        "ignore/on-the-field-inside-a-condition-less-inline-fragment",
        "subscription S {\n zoneUpdate {\n meta {\n ... {\n secret # graphox-ignore\n }\n }\n }\n}",
    ),
    // The field lives in the fragment, and the diagnostic is reported at the
    // spread in another definition; the comment still travels with it.
    (
        "ignore/on-the-field-inside-a-fragment",
        "subscription S {\n zoneUpdate {\n ...F\n }\n}\nfragment F on Zone {\n source {\n ... on ScheduleSource {\n schedule {\n id\n secret # graphox-ignore\n }\n }\n }\n}",
    ),
    (
        "ignore/on-a-top-level-field-of-a-spread-fragment",
        "subscription S {\n zoneUpdate {\n meta {\n ...M\n }\n }\n}\nfragment M on Meta {\n id\n secret # graphox-ignore\n}",
    ),
    (
        "ignore/at-the-spread-that-brings-it-in",
        "subscription S {\n zoneUpdate {\n ...F # graphox-ignore\n }\n}\nfragment F on Zone {\n source {\n ... on ScheduleSource {\n schedule {\n id\n secret\n }\n }\n }\n}",
    ),
    (
        "ignore/at-a-spread-that-narrows",
        "subscription S {\n zoneUpdate {\n source {\n ...M # graphox-ignore\n }\n }\n}\nfragment M on Manual {\n id\n secret\n}",
    ),
];

/// Strip the ignore comment from a case document, leaving the rest byte for
/// byte, so a suppression case can be run both ways.
fn without_the_comment(text: &str) -> String {
    text.lines()
        .map(|line| match line.find("# graphox-ignore") {
            Some(i) => line[..i].trim_end().to_string(),
            None => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
#[ntest::timeout(5000)]
fn forbidden_field_can_be_suppressed_on_every_shape() {
    let config = forbidden_secret_in_subscriptions();
    let schema = fixtures::syntax_matrix_schema().clone().validate().unwrap();

    let mut problems = Vec::new();
    for (label, text) in SUPPRESSED {
        let doc = create_doc("file:///matrix.graphql", text);
        let (found, noise) = split_diagnostics(&doc.get_semantic_diagnostics(
            &schema,
            &[],
            None,
            Some(&config),
            false,
            true,
        ));
        if !noise.is_empty() {
            problems.push(format!("  {label}: bad document: {noise:?}"));
            continue;
        }
        if found != 0 {
            problems.push(format!("  {label}: not suppressed"));
        }

        // Without its comment the same document has to report, or the case
        // proves nothing about suppression.
        let bare = without_the_comment(text);
        let doc = create_doc("file:///matrix.graphql", &bare);
        let (found_bare, noise_bare) = split_diagnostics(&doc.get_semantic_diagnostics(
            &schema,
            &[],
            None,
            Some(&config),
            false,
            true,
        ));
        if !noise_bare.is_empty() {
            problems.push(format!("  {label}: bad once stripped: {noise_bare:?}"));
        } else if found_bare == 0 {
            problems.push(format!(
                "  {label}: reports nothing even without the comment, so it tests nothing"
            ));
        }
    }
    assert!(problems.is_empty(), "suppression:\n{}", problems.join("\n"));
}

/// A spread is a leaf: nothing inside it to annotate, so a comment on one
/// covers everything it brings in, at whatever depth and wherever the spread is
/// written. That is what lets a shared fragment be silenced for one operation
/// instead of for every operation that spreads it.
const SPREAD_COVERS_WHAT_IT_BRINGS: &[(&str, &str)] = &[
    (
        "spread/in-the-operation",
        "subscription S {\n zoneUpdate {\n ...A # graphox-ignore\n }\n}\nfragment A on Zone {\n source {\n ... on ScheduleSource {\n schedule {\n id\n secret\n }\n }\n }\n}",
    ),
    (
        "spread/under-a-field-inside-a-fragment",
        "subscription S {\n zoneUpdate {\n ...A\n }\n}\nfragment A on Zone {\n source {\n ...B # graphox-ignore\n }\n}\nfragment B on PlayableSource {\n ... on ScheduleSource {\n schedule {\n id\n secret\n }\n }\n}",
    ),
    (
        "spread/at-a-fragments-top-level",
        "subscription S {\n zoneUpdate {\n ...A\n }\n}\nfragment A on Zone {\n ...B # graphox-ignore\n}\nfragment B on Zone {\n source {\n ... on ScheduleSource {\n schedule {\n id\n secret\n }\n }\n }\n}",
    ),
    (
        "spread/contributing-a-field-at-the-key",
        "subscription S {\n zoneUpdate {\n meta {\n ...A\n }\n }\n}\nfragment A on Meta {\n ...B # graphox-ignore\n}\nfragment B on Meta {\n id\n secret\n}",
    ),
    (
        "spread/two-hops-deep",
        "subscription S {\n zoneUpdate {\n ...A\n }\n}\nfragment A on Zone {\n source {\n ...B # graphox-ignore\n }\n}\nfragment B on PlayableSource {\n ... on ScheduleSource {\n schedule {\n ...C\n }\n }\n}\nfragment C on Schedule {\n id\n secret\n}",
    ),
];

#[test]
#[ntest::timeout(5000)]
fn an_ignored_spread_covers_everything_it_brings_in() {
    let config = forbidden_secret_in_subscriptions();
    let schema = fixtures::syntax_matrix_schema().clone().validate().unwrap();

    let mut problems = Vec::new();
    for (label, text) in SPREAD_COVERS_WHAT_IT_BRINGS {
        let doc = create_doc("file:///matrix.graphql", text);
        let (found, noise) = split_diagnostics(&doc.get_semantic_diagnostics(
            &schema,
            &[],
            None,
            Some(&config),
            false,
            true,
        ));
        if !noise.is_empty() {
            problems.push(format!("  {label}: bad document: {noise:?}"));
            continue;
        }
        if found != 0 {
            problems.push(format!("  {label}: not covered"));
        }

        let bare = without_the_comment(text);
        let doc = create_doc("file:///matrix.graphql", &bare);
        let (found_bare, _) = split_diagnostics(&doc.get_semantic_diagnostics(
            &schema,
            &[],
            None,
            Some(&config),
            false,
            true,
        ));
        if found_bare == 0 {
            problems.push(format!(
                "  {label}: reports nothing without the comment either, so it tests nothing"
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "ignored spread:\n{}",
        problems.join("\n")
    );
}

/// Two spreads can feed one response key. A comment on one must not speak for
/// the other — the suppression is recorded per selection rather than per key
/// precisely so it cannot.
#[test]
#[ntest::timeout(5000)]
fn an_ignored_spread_does_not_cover_its_siblings() {
    let config = forbidden_secret_in_subscriptions();
    let schema = fixtures::syntax_matrix_schema().clone().validate().unwrap();

    // B carries the comment and selects nothing forbidden; C selects `secret`
    // at the same key and carries none.
    let text = "subscription S {\n zoneUpdate {\n ...A\n }\n}\nfragment A on Zone {\n ...B # graphox-ignore\n ...C\n}\nfragment B on Zone {\n source {\n ... on ScheduleSource {\n schedule {\n id\n }\n }\n }\n}\nfragment C on Zone {\n source {\n ... on ScheduleSource {\n schedule {\n secret\n }\n }\n }\n}";
    let doc = create_doc("file:///matrix.graphql", text);
    let (found, noise) = split_diagnostics(&doc.get_semantic_diagnostics(
        &schema,
        &[],
        None,
        Some(&config),
        false,
        true,
    ));
    assert!(noise.is_empty(), "bad document: {noise:?}");
    assert_eq!(
        found, 1,
        "the comment on B silenced a finding that came from C"
    );
}

/// A scoped comment on a spread narrows what it covers, like anywhere else.
#[test]
#[ntest::timeout(5000)]
fn an_ignored_spread_respects_its_scope() {
    let config = forbidden_secret_in_subscriptions();
    let schema = fixtures::syntax_matrix_schema().clone().validate().unwrap();

    let text = "subscription S {\n zoneUpdate {\n ...A\n }\n}\nfragment A on Zone {\n source {\n ...B # graphox-ignore required_fields\n }\n}\nfragment B on PlayableSource {\n ... on ScheduleSource {\n schedule {\n id\n secret\n }\n }\n}";
    let doc = create_doc("file:///matrix.graphql", text);
    let (found, noise) = split_diagnostics(&doc.get_semantic_diagnostics(
        &schema,
        &[],
        None,
        Some(&config),
        false,
        true,
    ));
    assert!(noise.is_empty(), "bad document: {noise:?}");
    assert_eq!(found, 1, "a required_fields-only comment must leave this");
}

/// Placements that deliberately do *not* cover a forbidden finding. Every one
/// is a parent of the offending selection — an enclosing object, the field a
/// type condition hangs off, the inline fragment itself. A parent speaks for a
/// rule about a field that is not there to annotate, which is `required_fields`
/// and nothing else.
///
/// Silencing forbidden from a parent would take the whole subtree with it,
/// which is how one comment written for a deprecation warning used to switch
/// off the field rules on everything a union field selected.
const PARENTS_DO_NOT_COVER_FORBIDDEN: &[(&str, &str)] = &[
    (
        "parent/enclosing-object-in-an-operation",
        "subscription S {\n zoneUpdate {\n meta { # graphox-ignore\n secret\n }\n }\n}",
    ),
    (
        "parent/enclosing-object-inside-a-fragment",
        "subscription S {\n zoneUpdate {\n ...F\n }\n}\nfragment F on Zone {\n source {\n ... on ScheduleSource {\n schedule { # graphox-ignore\n id\n secret\n }\n }\n }\n}",
    ),
    (
        "parent/the-field-a-type-condition-hangs-off",
        "subscription S {\n zoneUpdate {\n source { # graphox-ignore\n ... on Manual {\n secret\n }\n }\n }\n}",
    ),
    (
        "parent/the-inline-fragment-itself",
        "subscription S {\n zoneUpdate {\n source {\n ... on Manual { # graphox-ignore\n secret\n }\n }\n }\n}",
    ),
    (
        "parent/a-condition-less-inline-fragment",
        "subscription S {\n zoneUpdate {\n meta {\n ... { # graphox-ignore\n secret\n }\n }\n }\n}",
    ),
    (
        "parent/one-level-further-out",
        "subscription S {\n zoneUpdate { # graphox-ignore\n meta {\n secret\n }\n }\n}",
    ),
    (
        "parent/a-member-narrowed-at-an-ignored-field",
        "subscription S {\n zoneUpdate {\n ...F\n }\n}\nfragment F on Zone {\n source { # graphox-ignore\n ...M\n }\n}\nfragment M on Manual {\n id\n secret\n}",
    ),
];

#[test]
#[ntest::timeout(5000)]
fn a_parent_does_not_silence_a_forbidden_field() {
    let config = forbidden_secret_in_subscriptions();
    let schema = fixtures::syntax_matrix_schema().clone().validate().unwrap();

    let mut over_suppressed = Vec::new();
    for (label, text) in PARENTS_DO_NOT_COVER_FORBIDDEN {
        let doc = create_doc("file:///matrix.graphql", text);
        let (found, noise) = split_diagnostics(&doc.get_semantic_diagnostics(
            &schema,
            &[],
            None,
            Some(&config),
            false,
            true,
        ));
        if !noise.is_empty() {
            over_suppressed.push(format!("  {label}: bad document: {noise:?}"));
        } else if found == 0 {
            over_suppressed.push(format!("  {label}: silenced from a parent"));
        }
    }
    assert!(
        over_suppressed.is_empty(),
        "a parent placement covered a forbidden field:\n{}",
        over_suppressed.join("\n")
    );
}

/// The mirror of the above for `required_fields`, which is the rule the parent
/// placement exists for: the field is absent, so there is nothing else to
/// annotate. The two rules reading the same comment differently is the point,
/// so both halves are pinned together.
#[test]
#[ntest::timeout(5000)]
fn a_parent_does_silence_a_required_field() {
    let schema = fixtures::syntax_matrix_schema().clone().validate().unwrap();
    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "secret".to_string(),
        RequiredFieldRule::new_operations(vec!["query".to_string()]),
    );
    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

    // `meta` selects no `secret`, which is the requirement; the comment on
    // `meta` is the only place that could carry the suppression.
    let text = "query Q {\n zone {\n secret\n meta { # graphox-ignore\n id\n }\n }\n}";
    let doc = create_doc("file:///matrix.graphql", text);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);
    let msgs: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
    assert!(
        msgs.is_empty(),
        "the parent placement must still work: {msgs:?}"
    );

    let bare = without_the_comment(text);
    let doc = create_doc("file:///matrix.graphql", &bare);
    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);
    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.starts_with("Required")),
        "without the comment this must report, or the case is vacuous"
    );
}

/// Fragment metadata is read from one of two places depending on whether the
/// definition is in this file or another, and the lookups have a branch for
/// each. Run the shapes that lean hardest on that metadata through the
/// workspace branch too.
const CROSS_FILE: &[(&str, &str, &str)] = &[
    (
        "xfile/nested-inline-union",
        r#"subscription S { zoneUpdate { ...F } }"#,
        r#"fragment F on Zone { source { ... on ScheduleSource { schedule { secret } } } }"#,
    ),
    (
        "xfile/root-inline-then-field",
        r#"subscription S { zoneUpdate { source { ...F } } }"#,
        r#"fragment F on PlayableSource { ... on ScheduleSource { schedule { secret } } }"#,
    ),
    (
        "xfile/nested-inline-spread-nesting",
        r#"subscription S { zoneUpdate { ...F } }"#,
        r#"fragment F on Zone { source { ... on ScheduleSource { ...SS } } }
           fragment SS on ScheduleSource { schedule { secret } }"#,
    ),
    (
        "xfile/no-type-condition",
        r#"subscription S { zoneUpdate { ...F } }"#,
        r#"fragment F on Zone { meta { ... { secret } } }"#,
    ),
    (
        "xfile/top-level-field",
        r#"subscription S { zoneUpdate { meta { ...F } } }"#,
        r#"fragment F on Meta { secret }"#,
    ),
    (
        "xfile/spread-chain",
        r#"subscription S { zoneUpdate { ...A } }"#,
        r#"fragment A on Zone { source { ...B } }
           fragment B on PlayableSource { ... on ScheduleSource { schedule { ...C } } }
           fragment C on Schedule { secret }"#,
    ),
];

#[test]
#[ntest::timeout(5000)]
fn forbidden_field_is_found_through_fragments_in_another_file() {
    let config = forbidden_secret_in_subscriptions();
    let schema = fixtures::syntax_matrix_schema().clone().validate().unwrap();

    let mut misses = Vec::new();
    for (label, op_text, fragment_text) in CROSS_FILE {
        let fragment_doc = create_doc("file:///other.graphql", fragment_text);
        let workspace = crate::support::workspace_fragments(&fragment_doc);
        let doc = create_doc("file:///matrix.graphql", op_text);
        let diagnostics =
            doc.get_semantic_diagnostics(&schema, &workspace, None, Some(&config), false, true);
        let (found, _) = split_diagnostics(&diagnostics);
        if found == 0 {
            misses.push(*label);
        }
    }
    assert!(
        misses.is_empty(),
        "shapes reported nothing when the fragment lives in another file:\n  {}",
        misses.join("\n  ")
    );
}

/// `name` exists only on Schedule, so a requirement on it fires once per
/// selected schedule and nowhere else. The interesting direction here is the
/// false positive: a route the merge cannot follow looks like a missing field,
/// and the only way out is to restructure the document.
const REQUIREMENT_SATISFIED: &[(&str, &str)] = &[
    (
        "sat/direct",
        r#"query Q { zone { meta { schedule { id name } } } }"#,
    ),
    (
        "sat/sibling-spread",
        r#"query Q { zone { meta { schedule { id ...N } } } }
           fragment N on Schedule { name }"#,
    ),
    (
        "sat/condition-less-inline",
        r#"query Q { zone { meta { schedule { id ... { name } } } } }"#,
    ),
    (
        "sat/stacked-condition-less-inline",
        r#"query Q { zone { meta { schedule { id ... { ... { name } } } } } }"#,
    ),
    (
        "sat/spread-inside-condition-less-inline",
        r#"query Q { zone { meta { schedule { id ... { ...N } } } } }
           fragment N on Schedule { name }"#,
    ),
    (
        "sat/redundant-type-condition",
        r#"query Q { zone { meta { schedule { id ... on Schedule { name } } } } }"#,
    ),
    (
        "sat/sibling-fragments-under-type-condition",
        r#"query Q { zone { ...A ...B } }
           fragment A on Zone { source { ... on ScheduleSource { schedule { id } } } }
           fragment B on Zone { source { ... on ScheduleSource { schedule { name } } } }"#,
    ),
    (
        "sat/spread-supplies-under-type-condition",
        r#"query Q { zone { source { ... on ScheduleSource { schedule { id ...N } } } } }
           fragment N on Schedule { name }"#,
    ),
];

const REQUIREMENT_MISSING: &[(&str, &str)] = &[
    (
        "miss/plain",
        r#"query Q { zone { meta { schedule { id } } } }"#,
    ),
    (
        "miss/under-type-condition",
        r#"query Q { zone { source { ... on ScheduleSource { schedule { id } } } } }"#,
    ),
    (
        "miss/in-fragment-under-type-condition",
        r#"query Q { zone { ...A } }
           fragment A on Zone { source { ... on ScheduleSource { schedule { id } } } }"#,
    ),
    (
        "miss/condition-less-inline",
        r#"query Q { zone { meta { schedule { ... { id } } } } }"#,
    ),
];

#[test]
#[ntest::timeout(5000)]
fn required_field_merges_across_every_shape() {
    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "name".to_string(),
        RequiredFieldRule::new_operations(vec!["query".to_string()]),
    );
    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));
    let schema = fixtures::syntax_matrix_schema().clone().validate().unwrap();

    let mut spurious = Vec::new();
    for (label, text) in REQUIREMENT_SATISFIED {
        let doc = create_doc("file:///matrix.graphql", text);
        let diagnostics =
            doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);
        if !diagnostics.is_empty() {
            let msgs: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
            spurious.push(format!("  {label}: {msgs:?}"));
        }
    }
    assert!(
        spurious.is_empty(),
        "the requirement is met by these documents but was still reported:\n{}",
        spurious.join("\n")
    );

    let mut unreported = Vec::new();
    for (label, text) in REQUIREMENT_MISSING {
        let doc = create_doc("file:///matrix.graphql", text);
        let diagnostics =
            doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);
        if !diagnostics.iter().any(|d| d.message.contains("Required")) {
            unreported.push(*label);
        }
    }
    assert!(
        unreported.is_empty(),
        "the requirement is genuinely unmet but nothing was reported:\n  {}",
        unreported.join("\n  ")
    );
}

/// GraphQL inside a template literal is parsed into its own tree at an offset,
/// so the node lookups behind these rules run on different inputs there.
#[test]
#[ntest::timeout(5000)]
fn field_rules_reach_shapes_inside_template_literals() {
    let cases: &[(&str, &str, usize)] = &[
        (
            "tsx/nested-inline-in-fragment",
            r#"const q = gql`
                subscription S { zoneUpdate { ...F } }
                fragment F on Zone { source { ... on ScheduleSource { schedule { secret } } } }
            `;"#,
            1,
        ),
        (
            "tsx/condition-less-inline",
            r#"const q = gql`
                subscription S { zoneUpdate { meta { ... { secret } } } }
            `;"#,
            1,
        ),
        (
            "tsx/type-condition-then-condition-less",
            r#"const q = gql`
                subscription S { zoneUpdate { source { ... on Manual { ... { secret } } } } }
            `;"#,
            1,
        ),
        (
            "tsx/nothing-forbidden",
            r#"const q = gql`
                subscription S { zoneUpdate { meta { id } } }
            `;"#,
            0,
        ),
    ];

    let config = forbidden_secret_in_subscriptions();
    let schema = fixtures::syntax_matrix_schema().clone().validate().unwrap();

    let mut wrong = Vec::new();
    for (label, text, expected) in cases {
        let doc = create_doc("file:///matrix.tsx", text);
        let diagnostics =
            doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);
        let (found, noise) = split_diagnostics(&diagnostics);
        if found != *expected || !noise.is_empty() {
            wrong.push(format!(
                "  {label}: got {found}, want {expected}; noise {noise:?}"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "template literal shapes:\n{}",
        wrong.join("\n")
    );
}

/// Type-scoped rules (`forbidden_fields: { Manual: { secret: true } }`) only
/// fire on the type a field was attributed to, so they are the sharp test of
/// attribution: a global rule matches whatever type the walk guessed, and hides
/// the mistake. Each case is the same selection reached a different way.
const TYPE_SCOPED: &[(&str, &str, &str, bool)] = &[
    (
        "typed/inline-member",
        "forbidden_fields:\n  Manual:\n    secret: true\n",
        r#"subscription S { zoneUpdate { source { ... on Manual { secret } } } }"#,
        true,
    ),
    (
        "typed/spread-narrows-member",
        "forbidden_fields:\n  Manual:\n    secret: true\n",
        r#"subscription S { zoneUpdate { source { ...M } } }
           fragment M on Manual { id secret }"#,
        true,
    ),
    (
        "typed/spread-narrows-through-abstract-fragment",
        "forbidden_fields:\n  Manual:\n    secret: true\n",
        r#"subscription S { zoneUpdate { source { ...F } } }
           fragment F on PlayableSource { __typename ...M }
           fragment M on Manual { id secret }"#,
        true,
    ),
    (
        "typed/condition-less-inline",
        "forbidden_fields:\n  Meta:\n    secret: true\n",
        r#"subscription S { zoneUpdate { meta { ... { secret } } } }"#,
        true,
    ),
    (
        "typed/condition-less-inline-under-a-member",
        "forbidden_fields:\n  Manual:\n    secret: true\n",
        r#"subscription S { zoneUpdate { source { ... on Manual { ... { secret } } } } }"#,
        true,
    ),
    (
        "typed/nested-in-fragment-under-a-type-condition",
        "forbidden_fields:\n  Schedule:\n    secret: true\n",
        r#"subscription S { zoneUpdate { ...F } }
           fragment F on Zone { source { ... on ScheduleSource { schedule { id secret } } } }"#,
        true,
    ),
    // A rule scoped to another type must stay silent, or the rule is really
    // just a global one wearing a type name.
    (
        "typed/other-type-via-spread",
        "forbidden_fields:\n  Schedule:\n    secret: true\n",
        r#"subscription S { zoneUpdate { source { ...M } } }
           fragment M on Manual { id secret }"#,
        false,
    ),
    (
        "typed/other-type-via-inline",
        "forbidden_fields:\n  Schedule:\n    secret: true\n",
        r#"subscription S { zoneUpdate { source { ... on Manual { secret } } } }"#,
        false,
    ),
];

#[test]
#[ntest::timeout(5000)]
fn type_scoped_rules_attribute_the_right_type() {
    let schema = fixtures::syntax_matrix_schema().clone().validate().unwrap();

    let mut wrong = Vec::new();
    for (label, yaml, text, should_fire) in TYPE_SCOPED {
        let docs = yaml_rust2::YamlLoader::load_from_str(yaml).unwrap();
        let rules = graphox::config::RulesConfig::from_yaml(&docs[0]).unwrap();
        let config = Config::default().with_rules(rules);
        let doc = create_doc("file:///matrix.graphql", text);
        let diagnostics =
            doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);
        let (found, noise) = split_diagnostics(&diagnostics);
        let fired = found > 0;
        if fired != *should_fire || !noise.is_empty() {
            wrong.push(format!(
                "  {label}: fired={fired}, want {should_fire}; noise {noise:?}"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "type-scoped rules attributed to the wrong type:\n{}",
        wrong.join("\n")
    );
}

/// Members of one abstract type are checked independently: neither suppression
/// nor a satisfied requirement may leak from one to another.
#[test]
#[ntest::timeout(5000)]
fn union_members_are_checked_independently() {
    let schema = fixtures::syntax_matrix_schema().clone().validate().unwrap();
    let forbidden = forbidden_secret_in_subscriptions();

    // Both members offend: both are reported.
    let both = r#"subscription S { zoneUpdate { source { ...M ...SS } } }
                  fragment M on Manual { id secret }
                  fragment SS on ScheduleSource { schedule { id secret } }"#;
    let doc = create_doc("file:///matrix.graphql", both);
    let (found, noise) = split_diagnostics(&doc.get_semantic_diagnostics(
        &schema,
        &[],
        None,
        Some(&forbidden),
        false,
        true,
    ));
    assert!(noise.is_empty(), "{noise:?}");
    assert_eq!(found, 2, "expected one finding per member, got {found}");

    // Suppressing one member leaves the other reported.
    let one_ignored = "subscription S {\n zoneUpdate {\n source {\n ...M # graphox-ignore\n ... on ScheduleSource {\n schedule {\n id\n secret\n }\n }\n }\n }\n}\nfragment M on Manual {\n id\n secret\n}";
    let doc = create_doc("file:///matrix.graphql", one_ignored);
    let (found, _) = split_diagnostics(&doc.get_semantic_diagnostics(
        &schema,
        &[],
        None,
        Some(&forbidden),
        false,
        true,
    ));
    assert_eq!(found, 1, "suppression leaked between members, got {found}");

    // One member supplying a required field does not cover another.
    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "secret".to_string(),
        RequiredFieldRule::new_operations(vec!["query".to_string()]),
    );
    let required =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));
    let uncovered = r#"query Q { zone { secret source { ...M ... on ScheduleSource { schedule { id } } } } }
                       fragment M on Manual { id secret }"#;
    let doc = create_doc("file:///matrix.graphql", uncovered);
    let diagnostics =
        doc.get_semantic_diagnostics(&schema, &[], None, Some(&required), false, true);
    let msgs: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
    assert!(
        msgs.iter()
            .any(|m| m.contains("must be selected in 'schedule'")),
        "the schedule requirement was covered by another member: {msgs:?}"
    );
}

/// The same member reached by two routes is one object, so what either route
/// selects satisfies the other's requirement.
#[test]
#[ntest::timeout(5000)]
fn selections_on_one_member_merge_across_routes() {
    let schema = fixtures::syntax_matrix_schema().clone().validate().unwrap();
    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "secret".to_string(),
        RequiredFieldRule::new_operations(vec!["query".to_string()]),
    );
    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

    let cases: &[(&str, &str)] = &[
        (
            "inline-plus-spread",
            r#"query Q { zone { secret source { ... on Manual { id } ...MP } } }
               fragment MP on Manual { secret }"#,
        ),
        (
            "two-spreads",
            r#"query Q { zone { secret source { ...MA ...MB } } }
               fragment MA on Manual { id }
               fragment MB on Manual { secret }"#,
        ),
    ];

    for (label, text) in cases {
        let doc = create_doc("file:///matrix.graphql", text);
        let diagnostics =
            doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);
        let msgs: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert!(msgs.is_empty(), "{label}: routes did not merge: {msgs:?}");
    }
}

/// Narrowing has a direction, and the matrix above only ever narrows downward.
/// A fragment written on a *supertype* of what is in effect selects for every
/// possible type of it, so its fields belong to the response key itself. Filing
/// them under the supertype hides them from every rule about the type actually
/// selected, and a field that is plainly there reads as missing.
///
/// `fragment X on Node` spread at an interface-typed field is one of the most
/// ordinary things in a GraphQL codebase, so this is a cheap way to be badly
/// wrong.
const NARROWING_DIRECTION: &[(&str, &str, &str, bool)] = &[
    (
        "direction/fragment-on-a-supertype",
        "required_fields:\n  Pet:\n    id: true\n",
        r#"query Q { pets { name ...NodeBits } }
           fragment NodeBits on Node { id }"#,
        false,
    ),
    (
        "direction/fragment-on-the-same-type",
        "required_fields:\n  Pet:\n    id: true\n",
        r#"query Q { pets { name ...PetBits } }
           fragment PetBits on Pet { id }"#,
        false,
    ),
    // A rule scoped to a member nothing selects has nothing to attach to, which
    // is deliberate: narrowing to one member is not a claim about the others.
    (
        "direction/rule-on-a-member-that-is-never-selected",
        "required_fields:\n  Cat:\n    id: true\n",
        r#"query Q { pets { name ...DogBits } }
           fragment DogBits on Dog { id }"#,
        false,
    ),
    (
        "direction/rule-on-the-member-that-is-selected",
        "required_fields:\n  Dog:\n    barks: true\n",
        r#"query Q { pets { name ...DogBits } }
           fragment DogBits on Dog { id }"#,
        true,
    ),
    (
        "direction/nothing-supplies-it",
        "required_fields:\n  Pet:\n    id: true\n",
        r#"query Q { pets { name } }"#,
        true,
    ),
    // One fragment reached under two different conditions belongs under both.
    (
        "direction/shared-supertype-fragment-under-two-members",
        "required_fields:\n  Cat:\n    id: true\n  Dog:\n    id: true\n",
        r#"query Q { search { ...Both } }
           fragment Both on Result { ...FDog ...FCat }
           fragment FDog on Dog { barks ...NodeBits }
           fragment FCat on Cat { meows ...NodeBits }
           fragment NodeBits on Node { id }"#,
        false,
    ),
    (
        "direction/shared-fragment-one-member-still-missing",
        "required_fields:\n  Cat:\n    id: true\n  Dog:\n    id: true\n",
        r#"query Q { search { ...Both } }
           fragment Both on Result { ...FDog ...FCat }
           fragment FDog on Dog { barks id }
           fragment FCat on Cat { meows }"#,
        true,
    ),
];

#[test]
#[ntest::timeout(5000)]
fn narrowing_only_happens_toward_a_subtype() {
    let schema = fixtures::supertype_schema().clone().validate().unwrap();

    let mut wrong = Vec::new();
    for (label, yaml, text, should_report) in NARROWING_DIRECTION {
        let docs = yaml_rust2::YamlLoader::load_from_str(yaml).unwrap();
        let rules = graphox::config::RulesConfig::from_yaml(&docs[0]).unwrap();
        let config = Config::default().with_rules(rules);
        let doc = create_doc("file:///matrix.graphql", text);
        let diagnostics =
            doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);
        let required: Vec<&str> = diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .filter(|m| m.starts_with("Required"))
            .collect();
        let noise: Vec<&str> = diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .filter(|m| !m.starts_with("Required"))
            .collect();
        if !noise.is_empty() {
            wrong.push(format!("  {label}: bad document: {noise:?}"));
        } else if required.is_empty() == *should_report {
            wrong.push(format!(
                "  {label}: reported {required:?}, want reported={should_report}"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "narrowing direction:\n{}",
        wrong.join("\n")
    );
}
