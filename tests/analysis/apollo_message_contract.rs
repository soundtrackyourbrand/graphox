//! Contract tests for the apollo-compiler diagnostic messages graphox matches on.
//!
//! apollo exposes no machine-readable diagnostic identity, so
//! `graphox_core::apollo_messages` matches message text. These tests trigger
//! each rule through apollo and assert the pattern still matches, so a reword
//! upstream fails here instead of silently changing which diagnostics reach
//! users. Every rule in `HANDLED_BY_GRAPHOX` must appear in `CASES`.

use apollo_compiler::{ExecutableDocument, Schema};
use graphox_core::apollo_messages::{self, HANDLED_BY_GRAPHOX, MessageRule};

const SCHEMA: &str = r#"
type Query { user(id: ID!): User, users(first: Int): [User] }
type User { id: ID! name: String pet: Pet }
type Pet { id: ID! owner: User }
"#;

/// A document that provokes the named rule, and the rendered diagnostics.
fn rendered(doc_src: &str) -> Vec<String> {
    let schema = Schema::parse_and_validate(SCHEMA, "s.graphql").unwrap();
    match ExecutableDocument::parse(&schema, doc_src, "d.graphql") {
        Ok(doc) => match doc.validate(&schema) {
            Ok(_) => Vec::new(),
            Err(e) => e.errors.iter().map(|d| d.to_string()).collect(),
        },
        Err(e) => e.errors.iter().map(|d| d.to_string()).collect(),
    }
}

/// (rule name, document that should produce it)
const CASES: &[(&str, &str)] = &[
    (
        "duplicate_operation",
        "query A { user(id:\"1\"){id} } query A { user(id:\"1\"){id} }",
    ),
    (
        "fragment_not_found",
        "query A { user(id:\"1\"){ ...Nope } }",
    ),
    (
        "fragment_unused",
        "fragment F on User { id } query A { user(id:\"1\"){id} }",
    ),
    ("unknown_field", "query A { user(id:\"1\"){ nope } }"),
    (
        "type_system_in_executable",
        "type Foo { a: String } query A { user(id:\"1\"){id} }",
    ),
    (
        "alias_type_conflict",
        "query A { a: user(id:\"1\"){id} a: users{id} }",
    ),
    (
        "alias_field_conflict",
        "query A { a: user(id:\"1\"){id} a: users{id} }",
    ),
    (
        "conflicting_arguments",
        "query A { a: user(id:\"1\"){id} a: user(id:\"2\"){id} }",
    ),
    (
        "recursive_fragment",
        "fragment F on User { ...F } query A { user(id:\"1\"){ ...F } }",
    ),
    (
        "unknown_directive",
        "query A { user(id:\"1\"){ id @nope } }",
    ),
    ("undefined_variable", "query A { user(id: $x){ id } }"),
    (
        "unused_variable",
        "query A($un: Int) { user(id:\"1\"){ id } }",
    ),
    (
        "variable_type_mismatch",
        "query A($x: String) { user(id: $x){ id } }",
    ),
];

fn rule(name: &str) -> &'static MessageRule {
    HANDLED_BY_GRAPHOX
        .iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("no rule named {name}"))
}

#[test]
fn every_rule_still_matches_a_real_apollo_message() {
    for (name, doc_src) in CASES {
        let r = rule(name);
        let reports = rendered(doc_src);
        assert!(
            !reports.is_empty(),
            "rule `{name}`: apollo produced no diagnostics for this document at all, \
             so the case no longer exercises the rule: {doc_src}"
        );
        let matched = reports.iter().any(|rep| {
            r.needles
                .iter()
                .all(|n| apollo_messages::summary(rep).contains(n))
        });
        assert!(
            matched,
            "rule `{name}` no longer matches any apollo summary line.\n  needles: {:?}\n  summaries: {:#?}",
            r.needles,
            reports
                .iter()
                .map(|r| apollo_messages::summary(r))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn every_rule_has_a_case() {
    for r in HANDLED_BY_GRAPHOX {
        assert!(
            CASES.iter().any(|(name, _)| *name == r.name),
            "rule `{}` has no case in CASES, so nothing proves it still matches",
            r.name
        );
    }
}

#[test]
fn matching_ignores_the_quoted_source() {
    // The rendered report quotes the user's query. A field or alias whose name
    // contains a pattern must not suppress an unrelated diagnostic.
    let reports = rendered("query A { unused: user(bogus: 1) { id } }");
    assert!(
        !reports.is_empty(),
        "expected a diagnostic for the bogus argument"
    );
    for rep in &reports {
        assert!(
            rep.contains("unused"),
            "precondition: the report should quote the alias, else this proves nothing"
        );
        assert!(
            !apollo_messages::is_handled_by_graphox(rep),
            "a diagnostic was suppressed by text quoted from the user's query:\n{rep}"
        );
    }
}
