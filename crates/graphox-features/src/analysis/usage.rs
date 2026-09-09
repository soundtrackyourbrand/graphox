//! Answers "who uses this?" across a workspace.
//!
//! Before changing a field you want to know what selects it, and before
//! removing one you want to know that nothing does. The schema says what
//! *could* be selected; this says what is, and by whom.
//!
//! A **consumer** is a definition — an operation or fragment — whose own body
//! selects the field. Usage is counted directly rather than transitively: an
//! operation that spreads a fragment is not counted as a consumer of the
//! fragment's fields, because the definition you would edit to stop selecting a
//! field is the one that names it.

use ahash::AHashMap;
use apollo_compiler::executable::{Selection, SelectionSet};
use apollo_compiler::validation::Valid;
use apollo_compiler::{ExecutableDocument, Schema};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::{Definition, DefinitionKind, DocumentSource};

/// How the workspace uses one field of one type.
#[derive(Debug, Clone)]
pub struct FieldUsage {
    pub type_name: String,
    pub field_name: String,
    /// Definitions that select it, distinct and sorted. Empty when the schema
    /// declares the field and nothing selects it.
    pub consumers: Vec<usize>,
    /// Projects those definitions belong to.
    pub projects: BTreeSet<usize>,
}

impl FieldUsage {
    pub fn is_unused(&self) -> bool {
        self.consumers.is_empty()
    }
}

/// How the workspace uses one type.
#[derive(Debug, Clone)]
pub struct TypeUsage {
    pub name: String,
    /// Definitions selecting anything on it, distinct and sorted.
    pub consumers: Vec<usize>,
    /// Fields the schema declares that nothing selects.
    pub unused_fields: usize,
    /// Fields the schema declares.
    pub declared_fields: usize,
}

#[derive(Debug, Default)]
pub struct Usage {
    pub definitions: Vec<Definition>,
    /// Every field of every object-like type the schema declares, whether or
    /// not anything selects it. Sorted by type then field.
    pub fields: Vec<FieldUsage>,
    pub types: Vec<TypeUsage>,
    pub unparsed: Vec<PathBuf>,
}

impl Usage {
    /// Fields of `type_name`, in the schema's declaration order position —
    /// sorted by name, since that is how they are looked up.
    pub fn fields_of<'a>(&'a self, type_name: &str) -> impl Iterator<Item = &'a FieldUsage> {
        self.fields.iter().filter(move |f| f.type_name == type_name)
    }
}

/// Walk a selection set, recording which definition selects which field of
/// which type. Spreads are not followed: the fragment records its own body.
fn walk(
    set: &SelectionSet,
    definition: usize,
    seen: &mut AHashMap<(String, String), BTreeSet<usize>>,
) {
    let parent = set.ty.to_string();
    for selection in &set.selections {
        match selection {
            Selection::Field(field) => {
                seen.entry((parent.clone(), field.name.to_string()))
                    .or_default()
                    .insert(definition);
                walk(&field.selection_set, definition, seen);
            }
            Selection::InlineFragment(inline) => walk(&inline.selection_set, definition, seen),
            Selection::FragmentSpread(_) => {}
        }
    }
}

/// Every field the schema declares that a *selection* can name, so a field
/// nothing selects is reported rather than simply absent.
///
/// Input objects are left out on purpose. Their members appear only in argument
/// values, which this walk does not read, so including them would report every
/// input field in the schema as unused and inflate the unused count of every
/// input type. Reporting those needs argument analysis, which this does not do.
fn declared_fields(schema: &Valid<Schema>) -> BTreeMap<String, Vec<String>> {
    use apollo_compiler::schema::ExtendedType;

    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (name, ty) in &schema.types {
        if name.starts_with("__") {
            continue;
        }
        let fields: Vec<String> = match ty {
            ExtendedType::Object(obj) => obj.fields.keys().map(|f| f.to_string()).collect(),
            ExtendedType::Interface(iface) => iface.fields.keys().map(|f| f.to_string()).collect(),
            _ => continue,
        };
        out.insert(name.to_string(), fields);
    }
    out
}

pub fn analyze(schema: &Valid<Schema>, docs: &[DocumentSource<'_>]) -> Usage {
    let mut usage = Usage::default();
    let mut seen: AHashMap<(String, String), BTreeSet<usize>> = AHashMap::default();

    for doc in docs {
        let parsed = match ExecutableDocument::parse(schema, doc.source, doc.path) {
            Ok(parsed) => parsed,
            // A document that fails to resolve every spread still carries fully
            // typed selection sets, which is all this walk reads.
            Err(with_errors) => with_errors.partial,
        };

        if parsed.operations.is_empty() && parsed.fragments.is_empty() {
            if !doc.source.trim().is_empty() {
                usage.unparsed.push(doc.path.to_path_buf());
            }
            continue;
        }

        let mut push = |name: String, kind: DefinitionKind| {
            usage.definitions.push(Definition {
                name,
                kind,
                path: doc.path.to_path_buf(),
                project_idx: doc.project_idx,
            });
            usage.definitions.len() - 1
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
            let idx = push(label, DefinitionKind::Operation);
            walk(&operation.selection_set, idx, &mut seen);
        }

        for (name, fragment) in parsed.fragments.iter() {
            let idx = push(name.to_string(), DefinitionKind::Fragment);
            walk(&fragment.selection_set, idx, &mut seen);
        }
    }

    // Report against the schema, not against what was selected, so a field
    // nothing uses is a row with no consumers rather than a missing row.
    let declared = declared_fields(schema);
    for (type_name, fields) in &declared {
        for field_name in fields {
            let consumers = seen
                .get(&(type_name.clone(), field_name.clone()))
                .cloned()
                .unwrap_or_default();
            let projects = consumers
                .iter()
                .map(|id| usage.definitions[*id].project_idx)
                .collect();
            usage.fields.push(FieldUsage {
                type_name: type_name.clone(),
                field_name: field_name.clone(),
                consumers: consumers.into_iter().collect(),
                projects,
            });
        }
    }

    // A selection on a type the schema does not declare cannot happen, but a
    // union or scalar can carry `__typename`, which `declared_fields` skips.
    for ((type_name, field_name), consumers) in seen {
        let known = declared
            .get(&type_name)
            .is_some_and(|fields| fields.contains(&field_name));
        if known {
            continue;
        }
        let projects = consumers
            .iter()
            .map(|id| usage.definitions[*id].project_idx)
            .collect();
        usage.fields.push(FieldUsage {
            type_name,
            field_name,
            consumers: consumers.into_iter().collect(),
            projects,
        });
    }

    usage.fields.sort_by(|a, b| {
        a.type_name
            .cmp(&b.type_name)
            .then(a.field_name.cmp(&b.field_name))
    });

    let mut by_type: BTreeMap<String, (BTreeSet<usize>, usize, usize)> = BTreeMap::new();
    for field in &usage.fields {
        let entry = by_type
            .entry(field.type_name.clone())
            .or_insert_with(|| (BTreeSet::new(), 0, 0));
        entry.0.extend(field.consumers.iter().copied());
        entry.1 += usize::from(field.is_unused());
        entry.2 += 1;
    }
    usage.types = by_type
        .into_iter()
        .map(|(name, (consumers, unused, declared))| TypeUsage {
            name,
            consumers: consumers.into_iter().collect(),
            unused_fields: unused,
            declared_fields: declared,
        })
        .collect();
    // Most-used first; a tie breaks by name so the order is stable.
    usage.types.sort_by(|a, b| {
        b.consumers
            .len()
            .cmp(&a.consumers.len())
            .then_with(|| a.name.cmp(&b.name))
    });

    usage
}
