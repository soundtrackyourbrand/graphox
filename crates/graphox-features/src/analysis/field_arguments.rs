//! The `field_arguments` rule: limits on what an argument of a field may be,
//! checked against the values a document writes itself.
//!
//! Two kinds of value are knowable from the document alone. A literal, as in
//! `tracks(first: 3000)`, is checked where it is written. A variable's default,
//! as in `$first: Int = 2500`, is checked per operation, following the variable
//! into every fragment the operation spreads, since a fragment does not know
//! the defaults of the operations that spread it. A variable without a default
//! gets its value from the code that sends the operation, which graphox does
//! not see.

use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use apollo_compiler::Node;
use apollo_compiler::ast::Value;
use apollo_compiler::executable::{
    ExecutableDocument, Field, Fragment, Operation, Selection, SelectionSet,
};
use graphox_core::config::{
    ArgumentLimit, FieldArgumentsRule, FieldCoordinate, LimitValue, Severity,
};
use std::collections::BTreeMap;
use std::ops::Range;
use std::sync::Arc;

pub const DIAGNOSTIC_CODE: &str = "field_argument_limit";

/// A value that breaks a limit, by where it is written in the block.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// Byte range of the value within the block's source text.
    pub range: Range<usize>,
    pub message: String,
    pub severity: Severity,
}

/// Checks one block: the literals its definitions write, and the defaults of
/// its operations' variables wherever those variables are passed to a limited
/// argument. `fragments` are the project's, used to follow spreads.
pub fn check_block(
    block: &ExecutableDocument,
    fragments: &HashMap<Arc<str>, Node<Fragment>>,
    rules: &BTreeMap<FieldCoordinate, FieldArgumentsRule>,
) -> Vec<Finding> {
    if rules.is_empty() {
        return Vec::new();
    }
    let mut findings = Vec::new();

    for operation in block.operations.iter() {
        literals(&operation.selection_set, rules, &mut findings);
        defaults(operation, block, fragments, rules, &mut findings);
    }
    for fragment in block.fragments.values() {
        literals(&fragment.selection_set, rules, &mut findings);
    }

    findings.sort_by_key(|f| f.range.start);
    findings
}

/// Literal arguments in a definition's own selections. A spread's literals
/// are checked in the fragment, once, rather than in every definition that
/// spreads it.
fn literals(
    selection_set: &SelectionSet,
    rules: &BTreeMap<FieldCoordinate, FieldArgumentsRule>,
    findings: &mut Vec<Finding>,
) {
    for selection in &selection_set.selections {
        match selection {
            Selection::Field(field) => {
                if let Some((coordinate, rule)) = rule_for(selection_set, field, rules) {
                    for argument in &field.arguments {
                        let Some(limit) = rule.limit(&argument.name) else {
                            continue;
                        };
                        if matches!(*argument.value, Value::Variable(_)) {
                            continue;
                        }
                        if let (Some(problem), Some(range)) =
                            (breaks(&argument.value, limit), span(&argument.value))
                        {
                            findings.push(Finding {
                                range,
                                message: with_reason(
                                    format!(
                                        "'{coordinate}({}:)' is {} here{problem}",
                                        argument.name,
                                        written(&argument.value)
                                    ),
                                    rule,
                                ),
                                severity: rule.severity(),
                            });
                        }
                    }
                }
                literals(&field.selection_set, rules, findings);
            }
            Selection::InlineFragment(inline) => literals(&inline.selection_set, rules, findings),
            Selection::FragmentSpread(_) => {}
        }
    }
}

/// The defaults of an operation's variables, wherever its selections, with
/// every fragment expanded, pass one to a limited argument. Reported once per
/// variable and argument, on the default.
fn defaults(
    operation: &Operation,
    block: &ExecutableDocument,
    fragments: &HashMap<Arc<str>, Node<Fragment>>,
    rules: &BTreeMap<FieldCoordinate, FieldArgumentsRule>,
    findings: &mut Vec<Finding>,
) {
    let defaults: HashMap<&str, &Node<Value>> = operation
        .variables
        .iter()
        .filter_map(|v| Some((v.name.as_str(), v.default_value.as_ref()?)))
        .collect();
    if defaults.is_empty() {
        return;
    }

    let mut walk = DefaultsWalk {
        block,
        fragments,
        rules,
        defaults: &defaults,
        expanding: Vec::new(),
        visited: HashSet::default(),
        reported: HashSet::default(),
        findings,
    };
    walk.selections(&operation.selection_set);
}

struct DefaultsWalk<'a, 'f> {
    block: &'a ExecutableDocument,
    fragments: &'a HashMap<Arc<str>, Node<Fragment>>,
    rules: &'a BTreeMap<FieldCoordinate, FieldArgumentsRule>,
    defaults: &'a HashMap<&'a str, &'a Node<Value>>,
    /// The fragments being expanded on the way here, outermost first.
    expanding: Vec<&'a str>,
    /// Fragments already expanded in this operation.
    visited: HashSet<&'a str>,
    /// (variable, coordinate, argument) already reported.
    reported: HashSet<(String, String, String)>,
    findings: &'f mut Vec<Finding>,
}

impl<'a> DefaultsWalk<'a, '_> {
    fn selections(&mut self, selection_set: &'a SelectionSet) {
        for selection in &selection_set.selections {
            match selection {
                Selection::Field(field) => {
                    self.field(selection_set, field);
                    self.selections(&field.selection_set);
                }
                Selection::InlineFragment(inline) => self.selections(&inline.selection_set),
                Selection::FragmentSpread(spread) => {
                    let name = spread.fragment_name.as_str();
                    // A fragment yields the same findings wherever the operation
                    // spreads it, so it is expanded once. That also stops a
                    // cycle, which validation rejects but a partial document
                    // can hold.
                    if !self.visited.insert(name) {
                        continue;
                    }
                    let fragment = self
                        .fragments
                        .get(name)
                        .or_else(|| self.block.fragments.get(name));
                    if let Some(fragment) = fragment {
                        self.expanding.push(name);
                        self.selections(&fragment.selection_set);
                        self.expanding.pop();
                    }
                }
            }
        }
    }

    fn field(&mut self, selection_set: &SelectionSet, field: &Field) {
        let Some((coordinate, rule)) = rule_for(selection_set, field, self.rules) else {
            return;
        };
        for argument in &field.arguments {
            let Value::Variable(variable) = &*argument.value else {
                continue;
            };
            let (Some(limit), Some(default)) = (
                rule.limit(&argument.name),
                self.defaults.get(variable.as_str()),
            ) else {
                continue;
            };
            let key = (
                variable.to_string(),
                coordinate.to_string(),
                argument.name.to_string(),
            );
            if self.reported.contains(&key) {
                continue;
            }
            if let (Some(problem), Some(range)) = (breaks(default, limit), span(default)) {
                self.reported.insert(key);
                let place = match self.expanding.last() {
                    Some(fragment) => format!(" in fragment '{fragment}'"),
                    None => String::new(),
                };
                self.findings.push(Finding {
                    range,
                    message: with_reason(
                        format!(
                            "'${variable}' defaults to {}, which is passed to \
                             '{coordinate}({}:)'{place}{problem}",
                            written(default),
                            argument.name
                        ),
                        rule,
                    ),
                    severity: rule.severity(),
                });
            }
        }
    }
}

/// The rule for a field, matched on the type it is selected on.
fn rule_for<'r>(
    selection_set: &SelectionSet,
    field: &Field,
    rules: &'r BTreeMap<FieldCoordinate, FieldArgumentsRule>,
) -> Option<(&'r FieldCoordinate, &'r FieldArgumentsRule)> {
    rules.get_key_value(&FieldCoordinate::new(
        selection_set.ty.as_str(),
        field.name.as_str(),
    ))
}

/// How a value breaks a limit, as the end of a sentence, or None when it
/// keeps to it. Only numbers are compared with a maximum.
fn breaks(value: &Value, limit: &ArgumentLimit) -> Option<String> {
    match limit {
        ArgumentLimit::Max(max) => {
            let number = match value {
                Value::Int(i) => i.as_str().parse::<f64>().ok(),
                Value::Float(f) => f.as_str().parse::<f64>().ok(),
                _ => None,
            }?;
            (number > *max as f64).then(|| format!(", above its limit of {max}"))
        }
        ArgumentLimit::Values(allowed) => {
            let allowed_here = allowed.iter().any(|candidate| match (candidate, value) {
                (LimitValue::Int(n), Value::Int(i)) => i.as_str().parse::<i64>().ok() == Some(*n),
                (LimitValue::String(s), Value::String(v)) => s == v.as_str(),
                (LimitValue::String(s), Value::Enum(e)) => s == e.as_str(),
                (LimitValue::Boolean(b), Value::Boolean(v)) => b == v,
                _ => false,
            });
            (!allowed_here).then(|| {
                let listed: Vec<String> = allowed.iter().map(ToString::to_string).collect();
                format!(", but it may only be {}", or_list(&listed))
            })
        }
    }
}

fn or_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
    }
}

fn written(value: &Value) -> String {
    value.to_string()
}

fn span(value: &Node<Value>) -> Option<Range<usize>> {
    let location = value.location()?;
    Some(location.offset()..location.end_offset())
}

fn with_reason(message: String, rule: &FieldArgumentsRule) -> String {
    match &rule.reason {
        Some(reason) => format!("{message}: {reason}"),
        None => message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apollo_compiler::Schema;

    const SCHEMA: &str = r#"
        type Query { playlist(id: ID!): Playlist album: Album }
        type Playlist { tracks(first: Int, market: String): TrackConnection! }
        type Album { tracks(first: Int, order: Order): TrackConnection! }
        type TrackConnection { total: Int }
        enum Order { ASC DESC }
    "#;

    fn rules() -> BTreeMap<FieldCoordinate, FieldArgumentsRule> {
        BTreeMap::from([
            (
                FieldCoordinate::new("Playlist", "tracks"),
                FieldArgumentsRule::new(vec![("first".to_string(), ArgumentLimit::Max(100))])
                    .with_reason("page through it instead"),
            ),
            (
                FieldCoordinate::new("Album", "tracks"),
                FieldArgumentsRule::new(vec![
                    (
                        "first".to_string(),
                        ArgumentLimit::Values(vec![LimitValue::Int(100), LimitValue::Int(500)]),
                    ),
                    (
                        "order".to_string(),
                        ArgumentLimit::Values(vec![LimitValue::String("ASC".to_string())]),
                    ),
                ])
                .with_severity(Severity::Warning),
            ),
        ])
    }

    /// The findings for `source`, with `others` as the project's fragments
    /// from other files, as (the text each finding points at, its message).
    fn check(source: &str, others: &str) -> Vec<(String, String, Severity)> {
        let schema = Schema::parse_and_validate(SCHEMA, "schema.graphql").unwrap();
        let block = ExecutableDocument::parse(&schema, source, "block.graphql")
            .unwrap_or_else(|e| e.partial);
        let mut fragments: HashMap<Arc<str>, Node<Fragment>> = HashMap::default();
        let other_doc = ExecutableDocument::parse(&schema, others, "others.graphql")
            .unwrap_or_else(|e| e.partial);
        for (name, fragment) in other_doc.fragments.iter().chain(block.fragments.iter()) {
            fragments.insert(Arc::from(name.as_str()), fragment.clone());
        }
        check_block(&block, &fragments, &rules())
            .into_iter()
            .map(|f| (source[f.range].to_string(), f.message, f.severity))
            .collect()
    }

    #[test]
    fn a_literal_above_the_maximum_is_reported_where_it_is_written() {
        let found = check(
            r#"query Q { playlist(id: "1") { tracks(first: 3000) { total } } }"#,
            "",
        );
        assert_eq!(
            found,
            vec![(
                "3000".to_string(),
                "'Playlist.tracks(first:)' is 3000 here, above its limit of 100: page through it instead"
                    .to_string(),
                Severity::Error
            )]
        );
    }

    #[test]
    fn a_literal_within_the_limit_is_not_reported() {
        assert!(
            check(
                r#"query Q { playlist(id: "1") { tracks(first: 100) { total } } }"#,
                ""
            )
            .is_empty()
        );
    }

    #[test]
    fn a_literal_outside_the_allowed_values_is_reported_at_its_severity() {
        let found = check(
            "query Q { album { tracks(first: 200, order: DESC) { total } } }",
            "",
        );
        assert_eq!(
            found,
            vec![
                (
                    "200".to_string(),
                    "'Album.tracks(first:)' is 200 here, but it may only be 100 or 500".to_string(),
                    Severity::Warning
                ),
                (
                    "DESC".to_string(),
                    "'Album.tracks(order:)' is DESC here, but it may only be ASC".to_string(),
                    Severity::Warning
                ),
            ]
        );
    }

    #[test]
    fn the_rule_matches_the_type_the_field_is_selected_on() {
        // 500 is above Playlist's maximum but one of Album's allowed values.
        let found = check("query Q { album { tracks(first: 500) { total } } }", "");
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_default_is_reported_on_the_operation_naming_the_fragment() {
        let found = check(
            r#"query Q($first: Int = 2500) { playlist(id: "1") { ...Tracks } }"#,
            "fragment Tracks on Playlist { tracks(first: $first) { total } }",
        );
        assert_eq!(
            found,
            vec![(
                "2500".to_string(),
                "'$first' defaults to 2500, which is passed to 'Playlist.tracks(first:)' in \
                 fragment 'Tracks', above its limit of 100: page through it instead"
                    .to_string(),
                Severity::Error
            )]
        );
    }

    #[test]
    fn the_same_fragment_is_judged_by_each_operations_default() {
        let fragment = "fragment Tracks on Playlist { tracks(first: $first) { total } }";
        assert!(
            check(
                r#"query Small($first: Int = 50) { playlist(id: "1") { ...Tracks } }"#,
                fragment
            )
            .is_empty()
        );
        assert_eq!(
            check(
                r#"query Big($first: Int = 500) { playlist(id: "1") { ...Tracks } }"#,
                fragment
            )
            .len(),
            1
        );
    }

    #[test]
    fn a_default_passed_twice_is_reported_once() {
        let found = check(
            r#"query Q($first: Int = 2500) {
                a: playlist(id: "1") { tracks(first: $first) { total } }
                b: playlist(id: "2") { tracks(first: $first) { total } }
            }"#,
            "",
        );
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn a_variable_without_a_default_is_left_to_the_caller() {
        assert!(
            check(
                r#"query Q($first: Int) { playlist(id: "1") { tracks(first: $first) { total } } }"#,
                ""
            )
            .is_empty()
        );
    }

    #[test]
    fn a_literal_in_a_spread_fragment_is_reported_in_the_fragment_only() {
        // The operation's block holds no literal; the fragment from another
        // file is checked when its own file is.
        let found = check(
            r#"query Q { playlist(id: "1") { ...Tracks } }"#,
            "fragment Tracks on Playlist { tracks(first: 3000) { total } }",
        );
        assert!(found.is_empty(), "{found:?}");
    }
}
