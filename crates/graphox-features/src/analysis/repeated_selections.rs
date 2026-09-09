//! Finds selections that recur across a workspace's operations and fragments.
//!
//! Two questions, answered from the same walk:
//!
//! - Does a selection duplicate a fragment that already exists? Those are drift:
//!   a field added to the fragment silently misses the hand-rolled copies.
//! - Does a group of fields recur often enough to deserve a fragment of its own?
//!
//! The unit of comparison is a *selection set*, keyed by the type it sits on,
//! and the unit of identity within one is a member's serialized form — so
//! `image { placeholder }` and `image { sizes { thumbnail } }` never merge, and
//! neither do two selections of the same field with different arguments.

use ahash::{AHashMap, AHashSet};
use apollo_compiler::executable::{Selection, SelectionSet};
use apollo_compiler::validation::Valid;
use apollo_compiler::{ExecutableDocument, Schema};
use graphox_core::config::{RepeatedSelectionKind, RepeatedSelectionsRule, Severity};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// A file to analyse, as its GraphQL source with host-language code masked out.
pub struct DocumentSource<'a> {
    pub path: &'a Path,
    pub project_idx: usize,
    pub source: &'a str,
}

#[derive(Debug, Clone)]
pub struct Options {
    /// How many members a group needs before it is worth reporting.
    pub min_fields: usize,
    /// How many distinct definitions must share a group before it is reported.
    /// Only applies to `groups`; a single site duplicating an existing fragment
    /// is already a finding.
    pub min_uses: usize,
    /// Fields the configuration mandates everywhere. They still belong to a
    /// group, but do not count toward `min_fields`: graphox put them there, so
    /// a group of nothing but `id` and `permissions` says nothing about intent.
    pub uncounted_fields: AHashSet<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            min_fields: 3,
            min_uses: 3,
            uncounted_fields: AHashSet::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionKind {
    Operation,
    Fragment,
}

#[derive(Debug, Clone)]
pub struct Definition {
    pub name: String,
    pub kind: DefinitionKind,
    pub path: PathBuf,
    pub project_idx: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Every site sits in one project, so a fragment can be extracted without
    /// crossing a package boundary.
    InProject,
    /// Sites span projects; extracting means moving the fragment somewhere both
    /// can import from.
    CrossProject,
}

#[derive(Debug, Clone)]
pub struct Site {
    /// Index into [`Analysis::definitions`].
    pub definition: usize,
    /// The group covers the whole selection set, rather than part of it.
    pub exact: bool,
    /// Byte range covering the members, for anchoring a diagnostic. Offsets are
    /// into the file's masked source, which preserves the real file's offsets.
    pub span: Option<(usize, usize)>,
}

/// A set of members repeatedly selected together on one type.
#[derive(Debug, Clone)]
pub struct Group {
    pub type_name: String,
    /// Serialized members, sorted, so the group has one canonical form.
    pub members: Vec<String>,
    /// Members that counted toward `min_fields`.
    pub counted: usize,
    pub sites: Vec<Site>,
    /// Distinct definitions the sites belong to, sorted.
    pub definitions: Vec<usize>,
    pub scope: Scope,
    /// A fragment whose body is exactly this group. The shape is not a missing
    /// fragment then, it is one that exists and is being re-inlined, which the
    /// overlap findings already describe.
    pub covered_by: Option<String>,
}

impl Group {
    /// Serialized bytes saved if every site but one spread a fragment instead.
    /// A rough ordering key, not a bundle-size claim.
    pub fn redundant_bytes(&self) -> usize {
        let width: usize = self.members.iter().map(|m| m.len() + 1).sum();
        width.saturating_mul(self.definitions.len().saturating_sub(1))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlapKind {
    /// The selection set is exactly the fragment's own selection set.
    Matches,
    /// The selection set contains everything the fragment selects, plus more.
    Extends,
}

/// A selection set that re-inlines a fragment that already exists.
#[derive(Debug, Clone)]
pub struct Overlap {
    pub fragment: String,
    pub type_name: String,
    pub kind: OverlapKind,
    pub site: Site,
    /// What the site selects beyond the fragment, for `Extends`.
    pub extra: Vec<String>,
    /// The fragment's own width, counting only members that describe the
    /// selection. What a threshold on this finding is about, and measured the
    /// same way as a group's, so an entry's `min_fields` means one thing.
    pub shared: usize,
}

#[derive(Debug, Default)]
pub struct Analysis {
    pub definitions: Vec<Definition>,
    pub groups: Vec<Group>,
    pub overlaps: Vec<Overlap>,
    /// Files whose GraphQL could not be parsed against the schema at all.
    pub unparsed: Vec<PathBuf>,
}

/// Signatures of members that do not count toward a threshold.
///
/// Whether a member counts is decided from its `Selection`, where the field
/// name is available; the later passes only have the serialized signature, and
/// matching that against configured field names would miss `alias: id` and
/// `permissions(scope: X)` — counting members the collection pass had not.
type UncountedSignatures = AHashSet<String>;

/// One selection set encountered during the walk.
struct Occurrence {
    type_name: String,
    /// Serialized member -> byte range of that member.
    members: BTreeMap<String, Option<(usize, usize)>>,
    definition: usize,
    /// This is the definition's own outermost selection set, so for a fragment
    /// it is that fragment's whole body.
    is_definition_root: bool,
}

/// Collapse a serialized selection onto one line so that formatting differences
/// between two files cannot make identical selections look distinct.
fn normalize(serialized: &str) -> String {
    let mut out = String::with_capacity(serialized.len());
    let mut pending_space = false;
    for ch in serialized.chars() {
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(ch);
    }
    out
}

fn selection_span(selection: &Selection) -> Option<(usize, usize)> {
    let location = match selection {
        Selection::Field(node) => node.location(),
        Selection::FragmentSpread(node) => node.location(),
        Selection::InlineFragment(node) => node.location(),
    };
    location.map(|span| (span.offset(), span.end_offset()))
}

/// The range covering every span in `spans`, so a diagnostic can point at the
/// whole group rather than at whichever member happened to come first.
fn covering_span(spans: impl Iterator<Item = (usize, usize)>) -> Option<(usize, usize)> {
    spans.reduce(|a, b| (a.0.min(b.0), a.1.max(b.1)))
}

/// The field name a member selects, when it is a plain field. Used to decide
/// whether a member counts toward `min_fields`.
fn member_field_name(selection: &Selection) -> Option<&str> {
    match selection {
        Selection::Field(field) => Some(field.name.as_str()),
        _ => None,
    }
}

fn collect_selection_sets(
    set: &SelectionSet,
    definition: usize,
    is_root: bool,
    opts: &Options,
    uncounted: &mut UncountedSignatures,
    out: &mut Vec<Occurrence>,
) {
    let mut members = BTreeMap::new();
    let mut counted = 0usize;

    for selection in &set.selections {
        let signature = normalize(&selection.serialize().no_indent().to_string());
        let is_uncounted =
            member_field_name(selection).is_some_and(|name| opts.uncounted_fields.contains(name));
        if is_uncounted {
            uncounted.insert(signature.clone());
        } else {
            counted += 1;
        }
        members.insert(signature, selection_span(selection));
    }

    // A set that is nothing but a single spread is already extracted.
    let only_a_spread = set.selections.len() == 1
        && matches!(set.selections.first(), Some(Selection::FragmentSpread(_)));

    if counted >= opts.min_fields && !only_a_spread {
        out.push(Occurrence {
            type_name: set.ty.to_string(),
            members,
            definition,
            is_definition_root: is_root,
        });
    }

    for selection in &set.selections {
        match selection {
            Selection::Field(field) => collect_selection_sets(
                &field.selection_set,
                definition,
                false,
                opts,
                uncounted,
                out,
            ),
            Selection::InlineFragment(inline) => {
                // An inline fragment narrows the type but does not open a new
                // definition, so its set is still the definition's own.
                collect_selection_sets(
                    &inline.selection_set,
                    definition,
                    is_root,
                    opts,
                    uncounted,
                    out,
                )
            }
            Selection::FragmentSpread(_) => {}
        }
    }
}

/// A fragment's own selection set, as the member signatures it is made of.
struct FragmentShape {
    name: String,
    type_name: String,
    members: BTreeSet<String>,
    definition: usize,
}

pub fn analyze(schema: &Valid<Schema>, docs: &[DocumentSource<'_>], opts: &Options) -> Analysis {
    let mut analysis = Analysis::default();
    let mut occurrences: Vec<Occurrence> = Vec::new();
    let mut fragment_shapes: Vec<FragmentShape> = Vec::new();
    let mut uncounted: UncountedSignatures = AHashSet::default();

    for doc in docs {
        let parsed = match ExecutableDocument::parse(schema, doc.source, doc.path) {
            Ok(parsed) => parsed,
            // A document that fails to resolve every spread still carries fully
            // typed selection sets, which is all this walk reads. Only a source
            // that yields nothing at all is reported as unparsed.
            Err(with_errors) => with_errors.partial,
        };

        if parsed.operations.is_empty() && parsed.fragments.is_empty() {
            if !doc.source.trim().is_empty() {
                analysis.unparsed.push(doc.path.to_path_buf());
            }
            continue;
        }

        let mut push_definition = |name: String, kind: DefinitionKind| {
            analysis.definitions.push(Definition {
                name,
                kind,
                path: doc.path.to_path_buf(),
                project_idx: doc.project_idx,
            });
            analysis.definitions.len() - 1
        };

        let operations = parsed
            .operations
            .anonymous
            .iter()
            .map(|op| (None, op))
            .chain(parsed.operations.named.iter().map(|(n, op)| (Some(n), op)));

        for (name, operation) in operations {
            let label = name
                .map(|n| n.to_string())
                .unwrap_or_else(|| "(anonymous)".to_string());
            let idx = push_definition(label, DefinitionKind::Operation);
            collect_selection_sets(
                &operation.selection_set,
                idx,
                true,
                opts,
                &mut uncounted,
                &mut occurrences,
            );
        }

        for (name, fragment) in parsed.fragments.iter() {
            let idx = push_definition(name.to_string(), DefinitionKind::Fragment);
            collect_selection_sets(
                &fragment.selection_set,
                idx,
                true,
                opts,
                &mut uncounted,
                &mut occurrences,
            );

            fragment_shapes.push(FragmentShape {
                name: name.to_string(),
                type_name: fragment.selection_set.ty.to_string(),
                members: fragment
                    .selection_set
                    .selections
                    .iter()
                    .map(|s| normalize(&s.serialize().no_indent().to_string()))
                    .collect(),
                definition: idx,
            });
        }
    }

    analysis.overlaps = find_overlaps(
        &occurrences,
        &fragment_shapes,
        &analysis.definitions,
        &uncounted,
    );
    analysis.groups = find_groups(
        &occurrences,
        &analysis.definitions,
        &fragment_shapes,
        &uncounted,
        opts,
    );
    analysis
}

/// Selection sets that re-inline a fragment defined elsewhere.
fn find_overlaps(
    occurrences: &[Occurrence],
    fragments: &[FragmentShape],
    definitions: &[Definition],
    uncounted: &UncountedSignatures,
) -> Vec<Overlap> {
    let mut by_type: AHashMap<&str, Vec<&FragmentShape>> = AHashMap::default();
    for shape in fragments {
        if shape.members.is_empty() {
            continue;
        }
        by_type.entry(&shape.type_name).or_default().push(shape);
    }

    let mut out = Vec::new();
    for occurrence in occurrences {
        let Some(candidates) = by_type.get(occurrence.type_name.as_str()) else {
            continue;
        };
        let members: BTreeSet<&String> = occurrence.members.keys().collect();

        for shape in candidates {
            // A fragment's own body is not a copy of itself.
            if shape.definition == occurrence.definition {
                continue;
            }
            let shape_members: BTreeSet<&String> = shape.members.iter().collect();
            if !shape_members.is_subset(&members) {
                continue;
            }
            let extra: Vec<String> = members
                .difference(&shape_members)
                .map(|m| (*m).clone())
                .collect();
            let kind = if extra.is_empty() {
                OverlapKind::Matches
            } else {
                OverlapKind::Extends
            };

            // Two fragments with identical bodies each duplicate the other, and
            // that is one finding, not two. Keep the half of the pair that
            // sorts first so the choice is stable.
            let site_definition = &definitions[occurrence.definition];
            if kind == OverlapKind::Matches
                && occurrence.is_definition_root
                && site_definition.kind == DefinitionKind::Fragment
                && (shape.name.as_str(), shape.definition)
                    < (site_definition.name.as_str(), occurrence.definition)
            {
                continue;
            }
            out.push(Overlap {
                fragment: shape.name.clone(),
                type_name: occurrence.type_name.clone(),
                kind,
                shared: counted_members(shape.members.iter(), uncounted),
                site: Site {
                    definition: occurrence.definition,
                    exact: extra.is_empty(),
                    span: covering_span(
                        shape_members
                            .iter()
                            .filter_map(|m| occurrence.members.get(*m).copied().flatten()),
                    ),
                },
                extra,
            });
        }
    }

    // Largest overlap first, then stably by fragment name.
    out.sort_by(|a, b| {
        b.extra
            .len()
            .cmp(&a.extra.len())
            .reverse()
            .then_with(|| a.fragment.cmp(&b.fragment))
    });
    out
}

/// How many members count toward a threshold. A mandated field is present
/// because graphox put it there, so it does not describe the selection.
///
/// Every threshold measures width this way, including the one on an overlap:
/// otherwise an entry's `min_fields` would mean one thing for a group and
/// another for a fragment, and whether a finding appeared would depend on the
/// thresholds of the *other* entries, through the width at which selection sets
/// are collected at all.
fn counted_members<'a>(
    members: impl Iterator<Item = &'a String>,
    uncounted: &UncountedSignatures,
) -> usize {
    members.filter(|m| !uncounted.contains(m.as_str())).count()
}

fn counted_width(members: &[String], uncounted: &UncountedSignatures) -> usize {
    counted_members(members.iter(), uncounted)
}

/// Field groups shared by enough definitions to be worth extracting.
fn find_groups(
    occurrences: &[Occurrence],
    definitions: &[Definition],
    fragments: &[FragmentShape],
    uncounted: &UncountedSignatures,
    opts: &Options,
) -> Vec<Group> {
    // `options_for_rules` sets this when nothing asks for groups. Every
    // candidate would be rejected after a workspace-wide scan, so stop first.
    if opts.min_uses == usize::MAX {
        return Vec::new();
    }

    let mut by_type: AHashMap<&str, Vec<&Occurrence>> = AHashMap::default();
    for occurrence in occurrences {
        by_type
            .entry(occurrence.type_name.as_str())
            .or_default()
            .push(occurrence);
    }

    let mut candidates: AHashMap<(String, Vec<String>), ()> = AHashMap::default();
    for (type_name, sets) in &by_type {
        // Candidate shapes are the pairwise intersections: any group shared by
        // three sets is also shared by two of them, so this reaches every group
        // worth considering without enumerating the powerset.
        for (i, a) in sets.iter().enumerate() {
            for b in sets.iter().skip(i + 1) {
                let shared: Vec<String> = a
                    .members
                    .keys()
                    .filter(|k| b.members.contains_key(*k))
                    .cloned()
                    .collect();
                if counted_width(&shared, uncounted) < opts.min_fields {
                    continue;
                }
                candidates.insert(((*type_name).to_string(), shared), ());
            }
        }
    }

    let mut groups: Vec<Group> = Vec::new();
    for (type_name, members) in candidates.into_keys() {
        let member_set: BTreeSet<&String> = members.iter().collect();
        let sets = match by_type.get(type_name.as_str()) {
            Some(sets) => sets,
            None => continue,
        };

        let mut sites = Vec::new();
        let mut definition_ids = BTreeSet::new();
        for occurrence in sets {
            if !member_set
                .iter()
                .all(|m| occurrence.members.contains_key(*m))
            {
                continue;
            }
            definition_ids.insert(occurrence.definition);
            sites.push(Site {
                definition: occurrence.definition,
                exact: occurrence.members.len() == members.len(),
                span: covering_span(
                    members
                        .iter()
                        .filter_map(|m| occurrence.members.get(m).copied().flatten()),
                ),
            });
        }

        if definition_ids.len() < opts.min_uses {
            continue;
        }

        let projects: BTreeSet<usize> = definition_ids
            .iter()
            .map(|id| definitions[*id].project_idx)
            .collect();

        let covered_by = fragments
            .iter()
            .find(|shape| {
                shape.type_name == type_name
                    && shape.members.len() == members.len()
                    && members.iter().all(|m| shape.members.contains(m))
            })
            .map(|shape| shape.name.clone());

        groups.push(Group {
            type_name,
            counted: counted_width(&members, uncounted),
            covered_by,
            members,
            sites,
            definitions: definition_ids.into_iter().collect(),
            scope: if projects.len() > 1 {
                Scope::CrossProject
            } else {
                Scope::InProject
            },
        });
    }

    // Report only maximal groups: `{id name}` and `{id name composerType}`
    // describe one duplication, and listing both buries the finding.
    groups.sort_by_key(|g| std::cmp::Reverse(g.members.len()));
    let mut kept: Vec<Group> = Vec::new();
    for group in groups {
        let subsumed = kept.iter().any(|k| {
            k.type_name == group.type_name
                && k.definitions.len() == group.definitions.len()
                && group.members.iter().all(|m| k.members.contains(m))
        });
        if !subsumed {
            kept.push(group);
        }
    }

    kept.sort_by(|a, b| {
        b.redundant_bytes()
            .cmp(&a.redundant_bytes())
            .then_with(|| a.type_name.cmp(&b.type_name))
            .then_with(|| a.members.cmp(&b.members))
    });
    kept
}

/// One more place a finding applies to, beyond the one it is anchored at.
#[derive(Debug, Clone)]
pub struct RelatedSite {
    pub path: PathBuf,
    pub span: Option<(usize, usize)>,
    /// The operation or fragment the site sits in.
    pub definition: String,
}

/// A rule violation, ready to be turned into a diagnostic by a caller that can
/// map a byte offset in the file back to a position.
#[derive(Debug, Clone)]
pub struct RuleFinding {
    pub path: PathBuf,
    /// Byte range into the file's masked source, when the parser recorded one.
    pub span: Option<(usize, usize)>,
    pub severity: Severity,
    pub code: &'static str,
    pub message: String,
    /// The other places the same finding covers. A recurring shape is one
    /// finding with many sites, not many findings.
    pub related: Vec<RelatedSite>,
}

/// The widest thresholds any entry asks for, so one walk can serve them all and
/// each entry filters the result down afterwards.
pub fn options_for_rules(
    rules: &[RepeatedSelectionsRule],
    uncounted_fields: AHashSet<String>,
) -> Options {
    Options {
        min_fields: rules.iter().map(|r| r.min_fields).min().unwrap_or(3),
        min_uses: rules
            .iter()
            .filter(|r| r.kind == RepeatedSelectionKind::NewFragment)
            .map(|r| r.min_uses)
            .min()
            .unwrap_or(usize::MAX),
        uncounted_fields,
    }
}

pub fn findings_for_rules(
    analysis: &Analysis,
    rules: &[RepeatedSelectionsRule],
) -> Vec<RuleFinding> {
    let mut out = Vec::new();

    for rule in rules {
        match rule.kind {
            RepeatedSelectionKind::MatchesFragment | RepeatedSelectionKind::ExtendsFragment => {
                let wanted = if rule.kind == RepeatedSelectionKind::MatchesFragment {
                    OverlapKind::Matches
                } else {
                    OverlapKind::Extends
                };
                for overlap in &analysis.overlaps {
                    if overlap.kind != wanted
                        || overlap.shared < rule.min_fields
                        || rule.ignores(&overlap.type_name)
                    {
                        continue;
                    }
                    let definition = &analysis.definitions[overlap.site.definition];
                    let message = if wanted == OverlapKind::Matches {
                        format!(
                            "This selection on {} is exactly fragment '{}'. Spread it instead.",
                            overlap.type_name, overlap.fragment
                        )
                    } else {
                        format!(
                            "This selection on {} contains everything fragment '{}' selects, plus {}. Spread it and keep the rest alongside.",
                            overlap.type_name,
                            overlap.fragment,
                            overlap.extra.join(" ")
                        )
                    };
                    out.push(RuleFinding {
                        path: definition.path.clone(),
                        span: overlap.site.span,
                        severity: rule.severity,
                        code: rule.kind.as_str(),
                        message,
                        related: Vec::new(),
                    });
                }
            }
            RepeatedSelectionKind::NewFragment => {
                for group in &analysis.groups {
                    if group.counted < rule.min_fields
                        || group.definitions.len() < rule.min_uses
                        || rule.ignores(&group.type_name)
                        // A shape a fragment already covers is not a missing
                        // fragment; matches_fragment reports those sites.
                        || group.covered_by.is_some()
                    {
                        continue;
                    }
                    let scope = match group.scope {
                        Scope::InProject => "this project",
                        Scope::CrossProject => "several projects",
                    };

                    // One finding per shape, carrying every site. A shape is
                    // one decision — extract this fragment or do not — and
                    // reporting it once per site turned a dozen of them into
                    // hundreds of diagnostics saying the same thing.
                    let mut sites: Vec<&Site> = group.sites.iter().collect();
                    sites.sort_by(|a, b| {
                        let (a_def, b_def) = (
                            &analysis.definitions[a.definition],
                            &analysis.definitions[b.definition],
                        );
                        a_def
                            .path
                            .cmp(&b_def.path)
                            .then_with(|| a.span.cmp(&b.span))
                    });
                    let Some((anchor, rest)) = sites.split_first() else {
                        continue;
                    };
                    let anchor_definition = &analysis.definitions[anchor.definition];

                    out.push(RuleFinding {
                        path: anchor_definition.path.clone(),
                        span: anchor.span,
                        severity: rule.severity,
                        code: rule.kind.as_str(),
                        message: format!(
                            "{} definitions across {} select {{ {} }} on {}. Consider a fragment.",
                            group.definitions.len(),
                            scope,
                            group.members.join(" "),
                            group.type_name
                        ),
                        related: rest
                            .iter()
                            .map(|site| {
                                let definition = &analysis.definitions[site.definition];
                                RelatedSite {
                                    path: definition.path.clone(),
                                    span: site.span,
                                    definition: definition.name.clone(),
                                }
                            })
                            .collect(),
                    });
                }
            }
        }
    }

    out.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.span.cmp(&b.span))
            .then_with(|| a.code.cmp(b.code))
    });
    out
}
