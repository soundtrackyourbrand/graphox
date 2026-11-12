//! The spec's field merging rule (`FieldsInSetCanMerge`), judged per operation
//! with every fragment it spreads, wherever that fragment is written.
//!
//! A server validates the whole document it receives, so two selections that
//! cannot merge fail the request even when they live in different fragments,
//! in different files, under different type conditions. apollo-compiler checks
//! the rule only for operations, and only over the fragments in the document
//! it is given.

use crate::apollo_messages;
use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use apollo_compiler::executable::{
    ExecutableDocument, Field, Fragment, OperationMap, Selection, SelectionSet,
};
use apollo_compiler::parser::{SourceMap, SourceSpan};
use apollo_compiler::validation::Valid;
use apollo_compiler::{Node, Schema};
use std::ops::Range;
use std::sync::Arc;

/// Selections in one operation that cannot merge.
#[derive(Debug, Clone)]
pub struct FieldConflict {
    /// apollo's report, quoting both selections and the type each returns.
    pub report: String,
    /// Byte range in the block's source text: the conflicting field when the
    /// block holds it, otherwise the spread through which the operation reaches
    /// the fragment that does.
    pub range: Range<usize>,
}

/// Checks every operation in `block`. `fragments` and `fragment_sources` are
/// the project's resolved fragments and the files they were parsed from; the
/// block's own fragment definitions take precedence over them.
pub fn conflicts(
    schema: &Valid<Schema>,
    block: &ExecutableDocument,
    fragments: &HashMap<Arc<str>, Node<Fragment>>,
    fragment_sources: &SourceMap,
) -> Vec<FieldConflict> {
    let mut found = Vec::new();
    for operation in block.operations.iter() {
        if !may_conflict(&operation.selection_set, block, fragments) {
            continue;
        }
        let mut document = block.clone();
        document.operations = OperationMap::from_one(operation.clone());
        for name in spread_closure(&operation.selection_set, block, fragments) {
            if !document.fragments.contains_key(name.as_ref())
                && let Some(fragment) = fragments.get(&name)
            {
                if let Some(file_id) = fragment.location().map(|l| l.file_id())
                    && let Some(file) = fragment_sources.get(&file_id)
                {
                    Arc::make_mut(&mut document.sources).insert(file_id, file.clone());
                }
                document
                    .fragments
                    .insert(fragment.name.clone(), fragment.clone());
            }
        }

        let Err(errors) = document.validate(schema) else {
            continue;
        };
        for diagnostic in errors.errors.iter() {
            let report = diagnostic.to_string();
            if !apollo_messages::is_field_merging(&report) {
                continue;
            }
            let location = apollo_compiler::diagnostic::ToCliReport::location(diagnostic.error);
            found.push(FieldConflict {
                report,
                range: anchor(location, block, operation, fragments),
            });
        }
    }
    found
}

/// Whether the selections could fail to merge. Selections that share a
/// response key merge when they select the same field with the same arguments
/// and directives and return the same type, and their own selections merge in
/// turn. Only an operation where that does not hold everywhere is handed to
/// apollo, whose validation runs every rule and costs far more than this walk.
/// Answers `true` whenever it cannot tell.
fn may_conflict(
    selection_set: &SelectionSet,
    block: &ExecutableDocument,
    fragments: &HashMap<Arc<str>, Node<Fragment>>,
) -> bool {
    may_conflict_among(&[selection_set], block, fragments, 0)
}

/// Selections nested deeper than this are taken for a recursive fragment and
/// left to apollo, which reports it.
const MAX_DEPTH: usize = 64;

/// `sets` are the selection sets that merge into one response object: the
/// operation's own, or those of every field selected under one response key.
fn may_conflict_among(
    sets: &[&SelectionSet],
    block: &ExecutableDocument,
    fragments: &HashMap<Arc<str>, Node<Fragment>>,
    depth: usize,
) -> bool {
    if depth > MAX_DEPTH {
        return true;
    }
    let mut by_key: HashMap<&str, Vec<&Node<Field>>> = HashMap::default();
    let mut expanded: HashSet<&str> = HashSet::default();
    let mut pending: Vec<&SelectionSet> = sets.to_vec();
    while let Some(set) = pending.pop() {
        for selection in &set.selections {
            match selection {
                Selection::Field(field) => by_key
                    .entry(field.response_key().as_str())
                    .or_default()
                    .push(field),
                Selection::InlineFragment(inline) => pending.push(&inline.selection_set),
                Selection::FragmentSpread(spread) => {
                    let name = spread.fragment_name.as_str();
                    if !expanded.insert(name) {
                        continue;
                    }
                    let Some(fragment) = block.fragments.get(name).or_else(|| fragments.get(name))
                    else {
                        return true;
                    };
                    pending.push(&fragment.selection_set);
                }
            }
        }
    }
    by_key.values().any(|fields| {
        let first = fields[0];
        fields[1..]
            .iter()
            .any(|other| !selects_the_same(first, other))
            || (fields
                .iter()
                .any(|f| !f.selection_set.selections.is_empty())
                && may_conflict_among(
                    &fields.iter().map(|f| &f.selection_set).collect::<Vec<_>>(),
                    block,
                    fragments,
                    depth + 1,
                ))
    })
}

fn selects_the_same(a: &Field, b: &Field) -> bool {
    a.name == b.name
        && a.arguments == b.arguments
        && a.directives == b.directives
        && a.definition.ty == b.definition.ty
}

/// Every fragment the selection set reaches through spreads, directly or
/// through other fragments.
fn spread_closure(
    selection_set: &SelectionSet,
    block: &ExecutableDocument,
    fragments: &HashMap<Arc<str>, Node<Fragment>>,
) -> HashSet<Arc<str>> {
    let mut reached = HashSet::default();
    let mut pending = Vec::new();
    direct_spreads(selection_set, &mut pending);
    while let Some(name) = pending.pop() {
        if !reached.insert(name.clone()) {
            continue;
        }
        let fragment = block
            .fragments
            .get(name.as_ref())
            .or_else(|| fragments.get(&name));
        if let Some(fragment) = fragment {
            direct_spreads(&fragment.selection_set, &mut pending);
        }
    }
    reached
}

/// Spreads written in this selection set and its nested ones, without
/// following them.
fn direct_spreads(selection_set: &SelectionSet, out: &mut Vec<Arc<str>>) {
    for selection in &selection_set.selections {
        match selection {
            Selection::Field(field) => direct_spreads(&field.selection_set, out),
            Selection::InlineFragment(inline) => direct_spreads(&inline.selection_set, out),
            Selection::FragmentSpread(spread) => out.push(Arc::from(spread.fragment_name.as_str())),
        }
    }
}

/// Where in the block a conflict reported at `location` belongs.
fn anchor(
    location: Option<SourceSpan>,
    block: &ExecutableDocument,
    operation: &Node<apollo_compiler::executable::Operation>,
    fragments: &HashMap<Arc<str>, Node<Fragment>>,
) -> Range<usize> {
    let whole_operation = || span(operation.location()).unwrap_or(0..0);
    let Some(location) = location else {
        return whole_operation();
    };
    if block.sources.contains_key(&location.file_id()) {
        return location.offset()..location.end_offset();
    }
    let Some(holder) = fragments.values().find(|f| {
        f.location().is_some_and(|l| {
            l.file_id() == location.file_id()
                && l.offset() <= location.offset()
                && location.end_offset() <= l.end_offset()
        })
    }) else {
        return whole_operation();
    };
    let mut visited = HashSet::default();
    spread_reaching(
        &operation.selection_set,
        holder.name.as_str(),
        block,
        fragments,
        &mut visited,
    )
    .unwrap_or_else(whole_operation)
}

/// The first spread in `selection_set`, or in a fragment the block defines,
/// through which `target` is reached. `visited` holds the block's fragments
/// already searched, which a recursive fragment would otherwise revisit forever.
fn spread_reaching<'b>(
    selection_set: &'b SelectionSet,
    target: &str,
    block: &'b ExecutableDocument,
    fragments: &HashMap<Arc<str>, Node<Fragment>>,
    visited: &mut HashSet<&'b str>,
) -> Option<Range<usize>> {
    for selection in &selection_set.selections {
        let found = match selection {
            Selection::Field(field) => {
                spread_reaching(&field.selection_set, target, block, fragments, visited)
            }
            Selection::InlineFragment(inline) => {
                spread_reaching(&inline.selection_set, target, block, fragments, visited)
            }
            Selection::FragmentSpread(spread) => {
                let name = spread.fragment_name.as_str();
                if let Some(local) = block.fragments.get(name) {
                    if visited.insert(name) {
                        spread_reaching(&local.selection_set, target, block, fragments, visited)
                    } else {
                        None
                    }
                } else if name == target
                    || spread_closure_of(name, block, fragments).contains(target)
                {
                    span(spread.location())
                } else {
                    None
                }
            }
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

fn spread_closure_of(
    name: &str,
    block: &ExecutableDocument,
    fragments: &HashMap<Arc<str>, Node<Fragment>>,
) -> HashSet<Arc<str>> {
    fragments
        .get(name)
        .map(|f| spread_closure(&f.selection_set, block, fragments))
        .unwrap_or_default()
}

fn span(location: Option<SourceSpan>) -> Option<Range<usize>> {
    location.map(|l| l.offset()..l.end_offset())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = "
        type Query { feed: [Entry] entry(id: ID!): Entry viewer: Author }
        union Entry = Article | Video
        type Author { id: ID! name: String friend: Author }
        type Article { id: ID! author: Author! title: String }
        type Video { id: ID! author: Author title: String }
    ";

    /// `fragments` stand for a project's fragments from other files.
    fn check(document: &str, fragments: &str) -> (bool, bool) {
        let schema = Schema::parse_and_validate(SCHEMA, "schema.graphql").unwrap();
        let resolved = ExecutableDocument::parse(&schema, fragments, "fragments.graphql")
            .unwrap_or_else(|e| e.partial);
        let fragments: HashMap<Arc<str>, Node<Fragment>> = resolved
            .fragments
            .iter()
            .map(|(name, f)| (Arc::from(name.as_str()), f.clone()))
            .collect();
        let block = ExecutableDocument::parse(&schema, document, "block.graphql")
            .unwrap_or_else(|e| e.partial);
        let operation = block.operations.iter().next().unwrap();

        let mut whole = block.clone();
        for fragment in fragments.values() {
            whole
                .fragments
                .insert(fragment.name.clone(), fragment.clone());
        }
        let apollo_finds = whole.validate(&schema).is_err_and(|e| {
            e.errors
                .iter()
                .any(|d| apollo_messages::is_field_merging(&d.to_string()))
        });
        let found = !conflicts(&schema, &block, &fragments, &resolved.sources).is_empty();
        assert_eq!(found, apollo_finds, "conflicts() disagrees with apollo");

        (
            may_conflict(&operation.selection_set, &block, &fragments),
            found,
        )
    }

    #[test]
    fn identical_selections_under_a_shared_key_are_not_handed_to_apollo() {
        let doc = "query Q { feed { ... on Article { id title } ... on Video { id title } } }";
        assert_eq!(check(doc, ""), (false, false));
    }

    #[test]
    fn a_nullability_difference_is_handed_to_apollo() {
        let doc =
            "query Q { feed { ... on Article { author { id } } ... on Video { author { id } } } }";
        assert_eq!(check(doc, ""), (true, true));
    }

    #[test]
    fn differing_arguments_are_handed_to_apollo() {
        let doc =
            r#"query Q { a: entry(id: "1") { __typename } a: entry(id: "2") { __typename } }"#;
        assert_eq!(check(doc, ""), (true, true));
    }

    #[test]
    fn an_alias_onto_another_field_is_handed_to_apollo() {
        let doc = "query Q { viewer { name: id name } }";
        assert_eq!(check(doc, ""), (true, true));
    }

    #[test]
    fn a_conflict_below_identical_fields_is_handed_to_apollo() {
        let doc = "query Q { viewer { friend { name } } viewer { friend { name: id } } }";
        assert_eq!(check(doc, ""), (true, true));
    }

    #[test]
    fn a_conflict_reached_through_a_fragment_from_another_file_is_handed_to_apollo() {
        let doc = "query Q { feed { ...EntryAuthor } }";
        let fragments = "fragment EntryAuthor on Entry {
            ... on Article { author { id } }
            ... on Video { author { id } }
        }";
        assert_eq!(check(doc, fragments), (true, true));
    }

    #[test]
    fn a_spread_of_an_unknown_fragment_is_handed_to_apollo() {
        let schema = Schema::parse_and_validate(SCHEMA, "schema.graphql").unwrap();
        let block = ExecutableDocument::parse(&schema, "query Q { viewer { ...Missing } }", "b")
            .unwrap_or_else(|e| e.partial);
        let operation = block.operations.iter().next().unwrap();
        assert!(may_conflict(
            &operation.selection_set,
            &block,
            &HashMap::default()
        ));
    }
}
