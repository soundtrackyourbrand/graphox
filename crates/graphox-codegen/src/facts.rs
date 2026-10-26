//! `graphql-facts.json`: what each operation and fragment of an output
//! directory selects, with its fragments expanded, for tools outside graphox
//! that check how documents are used. The format is documented in
//! `docs/facts.md`; a change to it bumps [`FACTS_VERSION`].

use ahash::AHashMap as HashMap;
use apollo_compiler::Node;
use apollo_compiler::ast::{OperationType, Value};
use apollo_compiler::executable::{
    self, DirectiveList, Fragment, Operation, Selection, SelectionSet,
};
use graphox_core::config::{
    ArgumentLimit, CodegenConfig, FieldArgumentsRule, FieldCoordinate, LimitValue, Severity,
};
use sonic_rs::{JsonValueMutTrait, Value as Json, json};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use crate::context::{FragmentGenerated, OperationGenerated};
use crate::entrypoint::GraphqlOverloads;

pub const FACTS_FILE_NAME: &str = "graphql-facts.json";

/// The version of the format, written at the top of the file. It changes when
/// a reader of the previous version could misread the new one.
pub const FACTS_VERSION: u32 = 1;

/// Fragments a spread can resolve to: the project's, then the document's own.
pub struct FragmentScope<'a> {
    pub project: &'a HashMap<Arc<str>, Node<Fragment>>,
    pub document: &'a apollo_compiler::collections::IndexMap<apollo_compiler::Name, Node<Fragment>>,
}

impl FragmentScope<'_> {
    fn get(&self, name: &str) -> Option<&Node<Fragment>> {
        self.project.get(name).or_else(|| self.document.get(name))
    }
}

/// The facts of one operation. `returned` is added when the directory's file
/// is written, since it depends on the other definitions there.
pub fn operation_facts(
    operation: &Operation,
    source: &str,
    document_name: &str,
    fragments: &FragmentScope,
) -> Json {
    let kind = match operation.operation_type {
        OperationType::Query => "query",
        OperationType::Mutation => "mutation",
        OperationType::Subscription => "subscription",
    };
    let variables: Vec<Json> = operation
        .variables
        .iter()
        .map(|variable| {
            let mut entry = json!({
                "name": variable.name.as_str(),
                "type": variable.ty.to_string(),
            });
            if let Some(default) = &variable.default_value {
                put(&mut entry, "default", value(default));
            }
            entry
        })
        .collect();

    let mut entry = json!({
        "kind": kind,
        "name": operation.name.as_ref().map(|n| n.as_str()),
        "document": document_name,
        "source": source,
        "type": operation.selection_set.ty.as_str(),
        "variables": variables,
    });
    insert_directives(&mut entry, &operation.directives);
    put(
        &mut entry,
        "selections",
        selections(&operation.selection_set, fragments, &mut Vec::new()),
    );
    entry
}

/// The facts of one fragment. `returned` is added as for an operation.
pub fn fragment_facts(
    fragment: &Fragment,
    source: &str,
    document_name: &str,
    fragments: &FragmentScope,
) -> Json {
    let mut entry = json!({
        "kind": "fragment",
        "name": fragment.name.as_str(),
        "document": document_name,
        "source": source,
        "type": fragment.selection_set.ty.as_str(),
    });
    insert_directives(&mut entry, &fragment.directives);
    let mut expanding = vec![fragment.name.as_str().to_string()];
    put(
        &mut entry,
        "selections",
        selections(&fragment.selection_set, fragments, &mut expanding),
    );
    entry
}

/// `expanding` holds the fragments being expanded on the way here, so a cycle,
/// which validation rejects but a partial document can still hold, stops
/// instead of recursing forever.
fn selections(
    selection_set: &SelectionSet,
    fragments: &FragmentScope,
    expanding: &mut Vec<String>,
) -> Vec<Json> {
    selection_set
        .selections
        .iter()
        .map(|selection| match selection {
            Selection::Field(field) => {
                let mut entry = json!({
                    "kind": "field",
                    "parent": selection_set.ty.as_str(),
                    "name": field.name.as_str(),
                    "type": field.definition.ty.to_string(),
                });
                if let Some(alias) = &field.alias {
                    put(&mut entry, "alias", alias.as_str());
                }
                if !field.arguments.is_empty() {
                    put(&mut entry, "arguments", arguments(&field.arguments));
                }
                insert_directives(&mut entry, &field.directives);
                if !field.selection_set.selections.is_empty() {
                    put(
                        &mut entry,
                        "selections",
                        selections(&field.selection_set, fragments, expanding),
                    );
                }
                entry
            }
            Selection::InlineFragment(inline) => {
                let mut entry = json!({ "kind": "inline" });
                if let Some(on) = &inline.type_condition {
                    put(&mut entry, "on", on.as_str());
                }
                insert_directives(&mut entry, &inline.directives);
                put(
                    &mut entry,
                    "selections",
                    selections(&inline.selection_set, fragments, expanding),
                );
                entry
            }
            Selection::FragmentSpread(spread) => {
                let name = spread.fragment_name.as_str();
                let mut entry = json!({ "kind": "spread", "fragment": name });
                insert_directives(&mut entry, &spread.directives);
                match fragments.get(name) {
                    Some(fragment) if !expanding.iter().any(|n| n == name) => {
                        put(&mut entry, "on", fragment.selection_set.ty.as_str());
                        expanding.push(name.to_string());
                        put(
                            &mut entry,
                            "selections",
                            selections(&fragment.selection_set, fragments, expanding),
                        );
                        expanding.pop();
                    }
                    _ => {
                        put(&mut entry, "unresolved", true);
                    }
                }
                entry
            }
        })
        .collect()
}

fn arguments(arguments: &[Node<executable::Argument>]) -> Vec<Json> {
    arguments
        .iter()
        .map(|argument| {
            json!({
                "name": argument.name.as_str(),
                "value": value(&argument.value),
            })
        })
        .collect()
}

fn insert_directives(entry: &mut Json, directives: &DirectiveList) {
    if directives.is_empty() {
        return;
    }
    let directives: Vec<Json> = directives
        .iter()
        .map(|directive| {
            let mut entry = json!({ "name": directive.name.as_str() });
            if !directive.arguments.is_empty() {
                put(&mut entry, "arguments", arguments(&directive.arguments));
            }
            entry
        })
        .collect();
    put(entry, "directives", directives);
}

/// A value as written, tagged by its kind. Numbers stay strings, as GraphQL
/// writes them, so no precision is lost on the way to a reader.
fn value(written: &Value) -> Json {
    match written {
        Value::Null => json!({ "kind": "null" }),
        Value::Variable(name) => json!({ "kind": "variable", "name": name.as_str() }),
        Value::Enum(name) => json!({ "kind": "enum", "value": name.as_str() }),
        Value::String(s) => json!({ "kind": "string", "value": s.as_str() }),
        Value::Boolean(b) => json!({ "kind": "boolean", "value": *b }),
        Value::Int(i) => json!({ "kind": "int", "value": i.as_str() }),
        Value::Float(f) => json!({ "kind": "float", "value": f.as_str() }),
        Value::List(items) => {
            let values: Vec<Json> = items.iter().map(|item| value(item)).collect();
            json!({ "kind": "list", "values": values })
        }
        Value::Object(fields) => {
            let fields: Vec<Json> = fields
                .iter()
                .map(|(name, field)| json!({ "name": name.as_str(), "value": value(field) }))
                .collect();
            json!({ "kind": "object", "fields": fields })
        }
    }
}

/// The `field_arguments` limits the directory's project declares, one entry
/// per field and argument, ordered by field.
pub type Limits = BTreeMap<FieldCoordinate, FieldArgumentsRule>;

fn limits(limits: &Limits) -> Vec<Json> {
    let mut entries = Vec::new();
    for (coordinate, rule) in limits {
        for (argument, limit) in &rule.arguments {
            let mut entry = json!({
                "field": coordinate.to_string(),
                "argument": argument.as_str(),
                "severity": match rule.severity() {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                    Severity::Info => "info",
                },
            });
            match limit {
                ArgumentLimit::Max(max) => put(&mut entry, "max", *max),
                ArgumentLimit::Values(values) => {
                    let values: Vec<Json> = values
                        .iter()
                        .map(|value| match value {
                            LimitValue::Int(i) => Json::from(*i),
                            LimitValue::String(s) => Json::from(s.as_str()),
                            LimitValue::Boolean(b) => Json::from(*b),
                        })
                        .collect();
                    put(&mut entry, "values", values);
                }
            }
            if let Some(reason) = &rule.reason {
                put(&mut entry, "reason", reason.as_str());
            }
            entries.push(entry);
        }
    }
    entries
}

/// The whole file: the version, the limits, then one definition per line,
/// operations first. Definitions codegen produced without facts are left out.
///
/// Each definition is marked `returned` when `graphql()` returns its document
/// for its source text, which only one of the definitions in a block is.
pub fn generate_facts_content(
    operations: &[OperationGenerated],
    fragments: &[FragmentGenerated],
    field_limits: &Limits,
) -> String {
    let overloads = GraphqlOverloads::new(operations, fragments);
    let definitions = operations
        .iter()
        .filter_map(|op| Some((op.facts.as_ref()?, &op.source_text, &op.document_name)))
        .chain(fragments.iter().filter_map(|frag| {
            Some((frag.facts.as_ref()?, &frag.source_text, &frag.document_name))
        }));

    let mut content = format!(
        "{{\"version\":{FACTS_VERSION},\"limits\":{},\"definitions\":[",
        sonic_rs::to_string(&limits(field_limits)).expect("facts serialize")
    );
    let mut first = true;
    for (facts, source, document_name) in definitions {
        let mut entry = facts.clone();
        let returned = overloads.document_for(source) == Some(document_name.as_str());
        put(&mut entry, "returned", returned);

        content.push_str(if first { "\n" } else { ",\n" });
        first = false;
        content.push_str(&sonic_rs::to_string(&entry).expect("facts serialize"));
    }
    if !first {
        content.push('\n');
    }
    content.push_str("]}\n");
    content
}

/// Writes `graphql-facts.json` into `output_dir` when `emit_facts` is on,
/// leaving it untouched when its content has not changed, and removes it
/// when the option is off so a file from before does not go stale.
pub fn write_facts(
    output_dir: &Path,
    operations: &[OperationGenerated],
    fragments: &[FragmentGenerated],
    codegen_config: &CodegenConfig,
    field_limits: &Limits,
) -> std::io::Result<()> {
    let path = output_dir.join(FACTS_FILE_NAME);
    if !codegen_config.emit_facts() {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let content = generate_facts_content(operations, fragments, field_limits);
    if std::fs::read_to_string(&path).is_ok_and(|existing| existing == content) {
        return Ok(());
    }
    std::fs::write(&path, content)
}

/// Every entry is built as an object by `json!`, so the key always lands.
fn put(entry: &mut Json, key: &str, value: impl Into<Json>) {
    if let Some(object) = entry.as_object_mut() {
        object.insert(key, value);
    }
}
