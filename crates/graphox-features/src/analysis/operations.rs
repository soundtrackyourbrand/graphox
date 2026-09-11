//! Answers "what does this operation cost to serve?" across a workspace.
//!
//! A gateway rejects on depth and charges on breadth, so the numbers that
//! decide whether an operation is affordable are properties of the request it
//! sends — not of the source it was written as. That distinction drives
//! everything here: selections are collected through fragment spreads into the
//! shape one response takes, and measured there.
//!
//! Counted per response key, as the spec collects fields: two spreads that both
//! select `id` on the same object describe one entry in the response and one
//! resolver call, so they count once.

use ahash::{AHashMap, AHashSet};
use apollo_compiler::executable::{Fragment, Operation, Selection, SelectionSet};
use apollo_compiler::schema::Type;
use apollo_compiler::validation::Valid;
use apollo_compiler::{ExecutableDocument, Node, Schema};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use super::DocumentSource;

/// Fragments an operation may spread, as the project resolving it sees them.
pub type VisibleFragments = AHashMap<Arc<str>, Node<Fragment>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationKind {
    Query,
    Mutation,
    Subscription,
}

impl OperationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            OperationKind::Query => "query",
            OperationKind::Mutation => "mutation",
            OperationKind::Subscription => "subscription",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "query" => Some(OperationKind::Query),
            "mutation" => Some(OperationKind::Mutation),
            "subscription" => Some(OperationKind::Subscription),
            _ => None,
        }
    }
}

impl From<apollo_compiler::executable::OperationType> for OperationKind {
    fn from(value: apollo_compiler::executable::OperationType) -> Self {
        use apollo_compiler::executable::OperationType;
        match value {
            OperationType::Query => OperationKind::Query,
            OperationType::Mutation => OperationKind::Mutation,
            OperationType::Subscription => OperationKind::Subscription,
        }
    }
}

/// What one operation asks the server for.
#[derive(Debug, Clone)]
pub struct OperationCost {
    pub name: String,
    pub kind: OperationKind,
    pub path: PathBuf,
    pub project_idx: usize,
    /// Longest chain of nested fields in the request, spreads followed.
    pub depth: usize,
    /// The same, with spreads left unfollowed. Below `depth` when the depth
    /// comes from a fragment rather than from the operation's own body, which
    /// is the difference between rewriting the operation and rewriting what it
    /// spreads.
    pub own_depth: usize,
    /// Fields the response contains, and so resolver calls the request costs.
    pub fields: usize,
    /// List-typed fields along one path, counting each wrapper: two means the
    /// response grows with the product of two page sizes.
    pub list_nesting: usize,
    /// The path `list_nesting` counts, empty when nothing on the path is a list.
    pub list_path: String,
    /// The path `depth` measures.
    pub deepest_path: String,
    /// Top-level fields, which the server resolves in parallel.
    pub root_fields: usize,
    pub variables: usize,
    /// Distinct fragments the request pulls in, directly or transitively.
    pub spreads: usize,
    /// A spread named a fragment that could not be resolved, so every count
    /// above is a lower bound. Without this an operation whose fragment is
    /// missing reads as a cheap one.
    pub partial: bool,
}

#[derive(Debug, Default)]
pub struct Analysis {
    pub operations: Vec<OperationCost>,
    /// Files holding GraphQL that did not parse against the schema. They
    /// contribute no operations, so a consumer needs to know they were skipped.
    pub unparsed: Vec<PathBuf>,
}

/// List wrappers in a field's type. `[[Track!]!]!` is two, and each one
/// multiplies the size of the response.
fn list_wrappers(ty: &Type) -> usize {
    match ty {
        Type::List(inner) | Type::NonNullList(inner) => 1 + list_wrappers(inner),
        Type::Named(_) | Type::NonNullNamed(_) => 0,
    }
}

/// One field of the collected response shape.
#[derive(Debug, Default)]
struct Collected {
    lists: usize,
    children: BTreeMap<String, Collected>,
}

struct Collector<'a> {
    /// Fragments the project can see, which is how a spread reaches a fragment
    /// defined in another file.
    visible: &'a VisibleFragments,
    /// Fragments defined in the file being read, for a project whose fragment
    /// metadata does not carry one.
    local: &'a ExecutableDocument,
    follow_spreads: bool,
    spreads: AHashSet<String>,
    partial: bool,
}

impl Collector<'_> {
    fn collect(
        &mut self,
        set: &SelectionSet,
        out: &mut BTreeMap<String, Collected>,
        visiting: &mut Vec<String>,
    ) {
        for selection in &set.selections {
            match selection {
                Selection::Field(field) => {
                    let key = field.alias.as_ref().unwrap_or(&field.name).to_string();
                    let node = out.entry(key).or_insert_with(|| Collected {
                        lists: list_wrappers(&field.definition.ty),
                        children: BTreeMap::new(),
                    });
                    self.collect(&field.selection_set, &mut node.children, visiting);
                }
                // Into the parent level: an inline fragment's fields are
                // collected under the same response keys as its siblings, so
                // two type conditions selecting `id` describe one entry.
                Selection::InlineFragment(inline) => {
                    self.collect(&inline.selection_set, out, visiting)
                }
                Selection::FragmentSpread(spread) => {
                    let name = spread.fragment_name.as_str();
                    self.spreads.insert(name.to_string());
                    if !self.follow_spreads {
                        continue;
                    }
                    let fragment = self
                        .visible
                        .get(name)
                        .or_else(|| self.local.fragments.get(&spread.fragment_name))
                        .cloned();
                    let Some(fragment) = fragment else {
                        self.partial = true;
                        continue;
                    };
                    // A fragment cycle cannot validate, but this reads
                    // documents that did not, so the guard has to be here.
                    if visiting.iter().any(|seen| seen == name) {
                        continue;
                    }
                    visiting.push(name.to_string());
                    self.collect(&fragment.selection_set, out, visiting);
                    visiting.pop();
                }
            }
        }
    }
}

#[derive(Debug, Default)]
struct Shape {
    depth: usize,
    fields: usize,
    lists: usize,
    deepest_path: String,
    list_path: String,
}

fn extend(key: &str, rest: &str) -> String {
    if rest.is_empty() {
        key.to_string()
    } else {
        format!("{key}.{rest}")
    }
}

fn measure(children: &BTreeMap<String, Collected>) -> Shape {
    let mut shape = Shape::default();
    for (key, node) in children {
        let inner = measure(&node.children);

        shape.fields += 1 + inner.fields;

        let depth = 1 + inner.depth;
        if depth > shape.depth {
            shape.depth = depth;
            shape.deepest_path = extend(key, &inner.deepest_path);
        }

        // The most-nested path need not be the deepest one, so it is tracked
        // separately rather than read off the deepest.
        let lists = node.lists + inner.lists;
        if lists > shape.lists {
            shape.lists = lists;
            shape.list_path = extend(key, &inner.list_path);
        }
    }
    shape
}

fn cost(
    name: String,
    operation: &Operation,
    doc: &DocumentSource<'_>,
    parsed: &ExecutableDocument,
    visible: &VisibleFragments,
) -> OperationCost {
    let mut collector = Collector {
        visible,
        local: parsed,
        follow_spreads: true,
        spreads: AHashSet::default(),
        partial: false,
    };
    let mut collected = BTreeMap::new();
    collector.collect(&operation.selection_set, &mut collected, &mut Vec::new());
    let shape = measure(&collected);

    let mut own = Collector {
        visible,
        local: parsed,
        follow_spreads: false,
        spreads: AHashSet::default(),
        partial: false,
    };
    let mut own_collected = BTreeMap::new();
    own.collect(
        &operation.selection_set,
        &mut own_collected,
        &mut Vec::new(),
    );

    OperationCost {
        name,
        kind: operation.operation_type.into(),
        path: doc.path.to_path_buf(),
        project_idx: doc.project_idx,
        depth: shape.depth,
        own_depth: measure(&own_collected).depth,
        fields: shape.fields,
        list_nesting: shape.lists,
        list_path: shape.list_path,
        deepest_path: shape.deepest_path,
        root_fields: collected.len(),
        variables: operation.variables.len(),
        spreads: collector.spreads.len(),
        partial: collector.partial,
    }
}

pub fn analyze(
    schema: &Valid<Schema>,
    docs: &[DocumentSource<'_>],
    visible: &VisibleFragments,
) -> Analysis {
    let mut analysis = Analysis::default();

    for doc in docs {
        let parsed = match ExecutableDocument::parse(schema, doc.source, doc.path) {
            Ok(parsed) => parsed,
            // A document whose spreads did not all resolve still carries fully
            // typed selection sets; what is missing is recorded as `partial` on
            // the operations that spread it.
            Err(with_errors) => with_errors.partial,
        };

        let named = parsed
            .operations
            .named
            .iter()
            .map(|(name, operation)| (name.to_string(), operation));
        let operations = parsed
            .operations
            .anonymous
            .iter()
            .map(|operation| ("(anonymous)".to_string(), operation))
            .chain(named);

        let mut measured = 0;
        for (name, operation) in operations {
            let cost = cost(name, operation, doc, &parsed, visible);
            // A selection set cannot be empty in valid GraphQL, so an operation
            // selecting nothing is what error recovery left behind rather than
            // a request anyone sends — the parser hands back an anonymous
            // operation for a file that is not GraphQL at all. Reporting it
            // would put a zero-cost row in the ranking and hide the real
            // problem, which is that the file did not parse.
            if cost.fields == 0 {
                continue;
            }
            analysis.operations.push(cost);
            measured += 1;
        }

        // A file of shared fragments has no operations by design, so only a
        // file that yielded neither is one this could not read.
        let has_fragments = parsed.fragments.iter().next().is_some();
        if measured == 0 && !has_fragments && !doc.source.trim().is_empty() {
            analysis.unparsed.push(doc.path.to_path_buf());
        }
    }

    analysis
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const SCHEMA: &str = "
        type Query { account: Account, zone: SoundZone }
        type Account { id: ID!, name: String, zones: [SoundZone!]!, grids: [[Cell!]!]! }
        type SoundZone { id: ID!, name: String, playlists: [Playlist!]! }
        type Playlist { id: ID!, tracks: [Track!]! }
        type Track { id: ID!, title: String }
        type Cell { id: ID! }
    ";

    fn analyze_source(source: &str, visible: &VisibleFragments) -> Analysis {
        let schema = Schema::parse(SCHEMA, "schema.graphql").unwrap();
        let schema = schema.validate().expect("schema should be valid");
        let docs = vec![DocumentSource {
            path: Path::new("query.graphql"),
            project_idx: 0,
            source,
        }];
        analyze(&schema, &docs, visible)
    }

    fn only(source: &str) -> OperationCost {
        let analysis = analyze_source(source, &VisibleFragments::default());
        assert_eq!(analysis.operations.len(), 1);
        analysis.operations.into_iter().next().unwrap()
    }

    #[test]
    fn depth_follows_a_spread_and_own_depth_does_not() {
        let cost = only(
            "query Q { account { ...AccountZones } }
             fragment AccountZones on Account { zones { playlists { tracks { id } } } }",
        );

        assert_eq!(cost.depth, 5, "account.zones.playlists.tracks.id");
        assert_eq!(cost.own_depth, 1, "the operation itself selects account");
        assert_eq!(cost.deepest_path, "account.zones.playlists.tracks.id");
        assert_eq!(cost.spreads, 1);
        assert!(!cost.partial);
    }

    #[test]
    fn a_fragment_in_another_file_resolves_through_the_project() {
        let shared = "fragment Shared on Account { zones { id } }";
        let schema = Schema::parse(SCHEMA, "schema.graphql").unwrap();
        let schema = schema.validate().expect("schema should be valid");
        let parsed = ExecutableDocument::parse(&schema, shared, "shared.graphql")
            .expect("fragment should parse");

        let mut visible = VisibleFragments::default();
        for (name, fragment) in parsed.fragments.iter() {
            visible.insert(Arc::from(name.as_str()), fragment.clone());
        }

        let analysis = analyze_source("query Q { account { ...Shared } }", &visible);
        let cost = &analysis.operations[0];

        assert_eq!(cost.depth, 3, "account.zones.id");
        assert!(!cost.partial);
    }

    #[test]
    fn a_spread_that_does_not_resolve_marks_the_operation_partial() {
        let cost = only("query Q { account { id ...Missing } }");

        assert!(cost.partial, "its numbers are a lower bound");
        assert_eq!(cost.spreads, 1, "the spread is still counted");
        assert_eq!(cost.fields, 2, "account and id");
    }

    #[test]
    fn the_same_field_reached_twice_is_one_field() {
        let cost = only(
            "query Q { account { id ...A ...B } }
             fragment A on Account { id name }
             fragment B on Account { id name }",
        );

        assert_eq!(cost.fields, 3, "account, id and name");
        assert_eq!(cost.root_fields, 1);
    }

    #[test]
    fn an_alias_is_its_own_field() {
        let cost = only("query Q { account { id, other: name, name } }");

        assert_eq!(cost.fields, 4, "account, id, other and name");
    }

    #[test]
    fn nested_lists_count_along_one_path() {
        let cost = only("query Q { account { zones { playlists { tracks { id } } } } }");

        assert_eq!(cost.list_nesting, 3, "zones, playlists and tracks");
        assert_eq!(cost.list_path, "account.zones.playlists.tracks");
    }

    #[test]
    fn a_list_of_lists_counts_each_wrapper() {
        let cost = only("query Q { account { grids { id } } }");

        assert_eq!(cost.list_nesting, 2, "`[[Cell!]!]!` is two wrappers");
    }

    #[test]
    fn a_request_with_no_list_has_no_list_path() {
        let cost = only("query Q { zone { id name } }");

        assert_eq!(cost.list_nesting, 0);
        assert_eq!(cost.list_path, "");
    }

    #[test]
    fn an_inline_fragment_collects_into_its_parent() {
        let cost = only("query Q { account { id ... on Account { id name } } }");

        assert_eq!(cost.fields, 3, "account, id and name");
    }

    #[test]
    fn root_fields_and_variables_are_counted() {
        let cost = only("query Q($a: ID, $b: ID) { account { id } zone { id } }");

        assert_eq!(cost.root_fields, 2);
        assert_eq!(cost.variables, 2);
        assert_eq!(cost.kind, OperationKind::Query);
    }

    #[test]
    fn a_cyclic_spread_terminates() {
        // Invalid GraphQL, so it arrives as a partial parse rather than not at
        // all, and the walk still has to come back.
        let analysis = analyze_source(
            "query Q { account { ...A } }
             fragment A on Account { id ...B }
             fragment B on Account { name ...A }",
            &VisibleFragments::default(),
        );

        let cost = &analysis.operations[0];
        assert_eq!(cost.depth, 2, "account, then its fields");
    }

    /// Shared fragments live in files of their own, which have no operations by
    /// design and must not read as files that failed to parse.
    #[test]
    fn a_file_of_fragments_alone_is_not_unparsed() {
        let analysis = analyze_source(
            "fragment OnlyThis on Account { id name }",
            &VisibleFragments::default(),
        );

        assert!(analysis.operations.is_empty());
        assert!(analysis.unparsed.is_empty());
    }

    #[test]
    fn a_file_that_does_not_parse_is_reported() {
        let analysis = analyze_source("this is not graphql {{{", &VisibleFragments::default());

        assert!(analysis.operations.is_empty());
        assert_eq!(analysis.unparsed, vec![PathBuf::from("query.graphql")]);
    }

    /// A field the schema does not declare is dropped from the selection set
    /// rather than costing the whole document, so the operation is still
    /// measured — on what remains of it.
    #[test]
    fn an_undeclared_field_does_not_lose_the_operation() {
        let analysis = analyze_source(
            "query Q { account { id nope } }",
            &VisibleFragments::default(),
        );

        assert_eq!(analysis.operations.len(), 1);
        assert!(analysis.unparsed.is_empty());
        assert_eq!(analysis.operations[0].fields, 2, "account and id");
    }
}
