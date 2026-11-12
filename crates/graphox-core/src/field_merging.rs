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
    ExecutableDocument, Fragment, OperationMap, Selection, SelectionSet,
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
