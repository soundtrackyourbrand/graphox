//! Answers "how many definitions spread this fragment?"
//!
//! It is the leverage of a fragment: a generated type that costs 8 KB and is
//! spread by twelve definitions is a different proposition from the same 8 KB
//! spread by one.
//!
//! Counted directly and per definition, as [`usage`](super::usage) counts a
//! consumer — the definition you would edit to stop spreading a fragment is the
//! one that names it, not the one that spreads something that spreads it.

use ahash::AHashMap;
use apollo_compiler::executable::{Selection, SelectionSet};
use apollo_compiler::validation::Valid;
use apollo_compiler::{ExecutableDocument, Schema};

use super::DocumentSource;

/// The distinct fragments one definition's body names.
fn named_in(set: &SelectionSet, out: &mut Vec<String>) {
    for selection in &set.selections {
        match selection {
            Selection::Field(field) => named_in(&field.selection_set, out),
            Selection::InlineFragment(inline) => named_in(&inline.selection_set, out),
            Selection::FragmentSpread(spread) => {
                let name = spread.fragment_name.as_str();
                if !out.iter().any(|seen| seen == name) {
                    out.push(name.to_string());
                }
            }
        }
    }
}

/// How many definitions spread each fragment, by fragment name.
///
/// Fragments nothing spreads are absent rather than zero, so a caller reporting
/// on a fragment reads a missing entry as none.
pub fn direct_counts(
    schema: &Valid<Schema>,
    docs: &[DocumentSource<'_>],
) -> AHashMap<String, usize> {
    let mut counts: AHashMap<String, usize> = AHashMap::default();

    for doc in docs {
        let parsed = match ExecutableDocument::parse(schema, doc.source, doc.path) {
            Ok(parsed) => parsed,
            // Selection sets survive a document whose spreads did not all
            // resolve, and a spread that names a missing fragment is still a
            // definition depending on it.
            Err(with_errors) => with_errors.partial,
        };

        let bodies = parsed
            .operations
            .anonymous
            .iter()
            .map(|operation| &operation.selection_set)
            .chain(
                parsed
                    .operations
                    .named
                    .iter()
                    .map(|(_, operation)| &operation.selection_set),
            )
            .chain(
                parsed
                    .fragments
                    .iter()
                    .map(|(_, fragment)| &fragment.selection_set),
            );

        for body in bodies {
            let mut named = Vec::new();
            named_in(body, &mut named);
            for name in named {
                *counts.entry(name).or_default() += 1;
            }
        }
    }

    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const SCHEMA: &str = "
        type Query { account: Account }
        type Account { id: ID!, name: String, zones: [SoundZone!]! }
        type SoundZone { id: ID! }
    ";

    fn counts(source: &str) -> AHashMap<String, usize> {
        let schema = Schema::parse(SCHEMA, "schema.graphql").unwrap();
        let schema = schema.validate().expect("schema should be valid");
        let docs = vec![DocumentSource {
            path: Path::new("query.graphql"),
            project_idx: 0,
            source,
        }];
        direct_counts(&schema, &docs)
    }

    #[test]
    fn each_definition_spreading_a_fragment_counts_once() {
        let counts = counts(
            "query A { account { ...Basics } }
             query B { account { ...Basics } }
             fragment Basics on Account { id name }",
        );

        assert_eq!(counts.get("Basics"), Some(&2));
    }

    #[test]
    fn spreading_the_same_fragment_twice_in_one_definition_counts_once() {
        let counts = counts(
            "query A { account { ...Basics zones { id } } ... on Query { account { ...Basics } } }
             fragment Basics on Account { id name }",
        );

        assert_eq!(counts.get("Basics"), Some(&1));
    }

    #[test]
    fn a_fragment_spread_by_a_fragment_is_counted() {
        let counts = counts(
            "query A { account { ...Outer } }
             fragment Outer on Account { id ...Inner }
             fragment Inner on Account { name }",
        );

        assert_eq!(counts.get("Outer"), Some(&1));
        assert_eq!(counts.get("Inner"), Some(&1), "spread by Outer, not by A");
    }

    #[test]
    fn a_fragment_nothing_spreads_is_absent() {
        let counts = counts("fragment Lonely on Account { id }");

        assert!(counts.get("Lonely").is_none());
    }
}
