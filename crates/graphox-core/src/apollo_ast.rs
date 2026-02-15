use crate::CodegenConfig;
use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use apollo_compiler::Node;
use apollo_compiler::ast::{self, OperationType, Type, Value as GqlValue};
use apollo_compiler::executable::{self, Selection};
use sonic_rs::{JsonValueMutTrait, Value, json};
use std::sync::Arc;

pub fn serialize_operation(
    operation: &executable::Operation,
    fragments: &HashMap<Arc<str>, Node<executable::Fragment>>,
    config: &CodegenConfig,
) -> Value {
    let mut definitions = Vec::with_capacity(16);

    // 1. Add the operation itself
    definitions.push(convert_operation(operation, fragments, config));

    // 2. Add all transitive fragments
    if !config.inline_fragments() {
        let mut used_fragments: HashSet<Arc<str>> = HashSet::default();
        collect_fragments(&operation.selection_set, fragments, &mut used_fragments);

        let mut sorted_fragments: Vec<_> = Vec::with_capacity(used_fragments.len());
        sorted_fragments.extend(used_fragments);
        sorted_fragments.sort_unstable();

        for frag_name in sorted_fragments {
            if let Some(frag) = fragments.get(&frag_name) {
                definitions.push(convert_fragment(frag, fragments, config));
            }
        }
    }

    json!({
        "definitions": definitions,
        "kind": "Document",
    })
}

pub fn serialize_operation_definition(
    operation: &executable::Operation,
    all_fragments: &HashMap<Arc<str>, Node<executable::Fragment>>,
    config: &CodegenConfig,
) -> Value {
    convert_operation(operation, all_fragments, config)
}

pub fn serialize_fragment_definition(
    fragment: &executable::Fragment,
    all_fragments: &HashMap<Arc<str>, Node<executable::Fragment>>,
    config: &CodegenConfig,
) -> Value {
    convert_fragment(fragment, all_fragments, config)
}

pub fn get_operation_fragment_dependencies(
    operation: &executable::Operation,
    all_fragments: &HashMap<Arc<str>, Node<executable::Fragment>>,
) -> HashSet<Arc<str>> {
    let mut used: HashSet<Arc<str>> = HashSet::default();
    collect_fragments(&operation.selection_set, all_fragments, &mut used);
    used
}

pub fn get_fragment_fragment_dependencies(
    fragment: &executable::Fragment,
    all_fragments: &HashMap<Arc<str>, Node<executable::Fragment>>,
) -> HashSet<Arc<str>> {
    let mut used: HashSet<Arc<str>> = HashSet::default();
    collect_fragments(&fragment.selection_set, all_fragments, &mut used);
    used
}

fn collect_fragments(
    selection_set: &executable::SelectionSet,
    all_fragments: &HashMap<Arc<str>, Node<executable::Fragment>>,
    used: &mut HashSet<Arc<str>>,
) {
    for selection in &selection_set.selections {
        match selection {
            Selection::Field(field) => {
                collect_fragments(&field.selection_set, all_fragments, used);
            }
            Selection::InlineFragment(inline) => {
                collect_fragments(&inline.selection_set, all_fragments, used);
            }
            Selection::FragmentSpread(spread) => {
                let name = spread.fragment_name.as_str();
                if used.insert(Arc::from(name))
                    && let Some(frag) = all_fragments.get(name)
                {
                    collect_fragments(&frag.selection_set, all_fragments, used);
                }
            }
        }
    }
}

fn convert_operation(
    op: &executable::Operation,
    all_fragments: &HashMap<Arc<str>, Node<executable::Fragment>>,
    config: &CodegenConfig,
) -> Value {
    let mut map_val = json!({});
    {
        let map = map_val.as_object_mut().unwrap();
        map.insert("kind", json!("OperationDefinition"));
        if let Some(n) = &op.name {
            map.insert("name", convert_name(n.as_str()));
        }
        map.insert(
            "operation",
            json!(match op.operation_type {
                OperationType::Query => "query",
                OperationType::Mutation => "mutation",
                OperationType::Subscription => "subscription",
            }),
        );
        if !op.variables.is_empty() {
            map.insert(
                "variableDefinitions",
                json!(
                    op.variables
                        .iter()
                        .map(|v| convert_variable_def(v, config))
                        .collect::<Vec<_>>()
                ),
            );
        }
        map.insert(
            "selectionSet",
            convert_selection_set(&op.selection_set, all_fragments, config),
        );
        if config.emit_ast_directives() && !op.directives.is_empty() {
            let directive_vals: Vec<_> = op
                .directives
                .iter()
                .filter_map(|d| convert_directive(d, config))
                .collect();
            if !directive_vals.is_empty() {
                map.insert("directives", json!(directive_vals));
            }
        }
    }
    map_val
}

fn convert_fragment(
    frag: &executable::Fragment,
    all_fragments: &HashMap<Arc<str>, Node<executable::Fragment>>,
    config: &CodegenConfig,
) -> Value {
    let mut map_val = json!({});
    {
        let map = map_val.as_object_mut().unwrap();
        map.insert("kind", json!("FragmentDefinition"));
        map.insert("name", convert_name(frag.name.as_str()));
        map.insert(
            "selectionSet",
            convert_selection_set(&frag.selection_set, all_fragments, config),
        );
        map.insert(
            "typeCondition",
            json!({
                "kind": "NamedType",
                "name": convert_name(frag.type_condition().as_str()),
            }),
        );
        if config.emit_ast_directives() && !frag.directives.is_empty() {
            let directive_vals: Vec<_> = frag
                .directives
                .iter()
                .filter_map(|d| convert_directive(d, config))
                .collect();
            if !directive_vals.is_empty() {
                map.insert("directives", json!(directive_vals));
            }
        }
    }
    map_val
}

fn convert_selection_set(
    sel: &executable::SelectionSet,
    all_fragments: &HashMap<Arc<str>, Node<executable::Fragment>>,
    config: &CodegenConfig,
) -> Value {
    let mut selections = Vec::with_capacity(sel.selections.len());

    for selection in &sel.selections {
        match selection {
            Selection::Field(_) | Selection::InlineFragment(_) => {
                selections.push(convert_selection(selection, all_fragments, config));
            }
            Selection::FragmentSpread(spread) => {
                if config.inline_fragments() {
                    let frag_name = spread.fragment_name.as_str();
                    if let Some(frag) = all_fragments.get(frag_name) {
                        if config.emit_ast_directives() && !spread.directives.is_empty() {
                            // If there are directives on the spread, we MUST wrap in an InlineFragment
                            // to preserve them during inlining.
                            let mut inline_frag = json!({
                                "kind": "InlineFragment",
                                "typeCondition": {
                                    "kind": "NamedType",
                                    "name": convert_name(frag.type_condition().as_str()),
                                },
                                "selectionSet": convert_selection_set(&frag.selection_set, all_fragments, config),
                            });

                            let directive_vals: Vec<_> = spread
                                .directives
                                .iter()
                                .filter_map(|d| convert_directive(d, config))
                                .collect();

                            if !directive_vals.is_empty() {
                                inline_frag
                                    .as_object_mut()
                                    .unwrap()
                                    .insert("directives", json!(directive_vals));
                            }
                            selections.push(inline_frag);
                        } else {
                            // Otherwise, just flatten the selections to match manual inlining
                            for frag_selection in &frag.selection_set.selections {
                                selections.push(convert_selection(
                                    frag_selection,
                                    all_fragments,
                                    config,
                                ));
                            }
                        }
                    }
                } else {
                    selections.push(convert_selection(selection, all_fragments, config));
                }
            }
        }
    }

    json!({
        "kind": "SelectionSet",
        "selections": selections,
    })
}

// The helper functions for deep nested fragment expansion and field key
// extraction were removed because they were not referenced by the current
// selection conversion logic. Keeping unused helpers produced warnings and
// added maintenance burden. If deep expansion/deduplication is required in
// the future, reintroduce the helpers with a clear call-site in
// `convert_selection_set`.

fn convert_selection(
    sel: &Selection,
    all_fragments: &HashMap<Arc<str>, Node<executable::Fragment>>,
    config: &CodegenConfig,
) -> Value {
    match sel {
        Selection::Field(f) => {
            let mut map_val = json!({});
            {
                let map = map_val.as_object_mut().unwrap();
                map.insert("kind", json!("Field"));
                map.insert("name", convert_name(f.name.as_str()));
                if !f.selection_set.selections.is_empty() {
                    map.insert(
                        "selectionSet",
                        convert_selection_set(&f.selection_set, all_fragments, config),
                    );
                }
                if let Some(alias) = &f.alias
                    && config.emit_ast_aliases()
                {
                    map.insert("alias", convert_name(alias.as_str()));
                }
                if config.emit_ast_arguments() && !f.arguments.is_empty() {
                    map.insert(
                        "arguments",
                        json!(
                            f.arguments
                                .iter()
                                .map(|a| convert_argument(a))
                                .collect::<Vec<_>>()
                        ),
                    );
                }
                if config.emit_ast_directives() && !f.directives.is_empty() {
                    let directive_vals: Vec<_> = f
                        .directives
                        .iter()
                        .filter_map(|d| convert_directive(d, config))
                        .collect();
                    if !directive_vals.is_empty() {
                        map.insert("directives", json!(directive_vals));
                    }
                }
            }
            map_val
        }
        Selection::InlineFragment(f) => {
            let mut map_val = json!({});
            {
                let map = map_val.as_object_mut().unwrap();
                map.insert("kind", json!("InlineFragment"));
                map.insert(
                    "selectionSet",
                    convert_selection_set(&f.selection_set, all_fragments, config),
                );
                if let Some(t) = &f.type_condition {
                    map.insert(
                        "typeCondition",
                        json!({
                            "kind": "NamedType",
                            "name": convert_name(t.as_str()),
                        }),
                    );
                }
                if config.emit_ast_directives() && !f.directives.is_empty() {
                    let directive_vals: Vec<_> = f
                        .directives
                        .iter()
                        .filter_map(|d| convert_directive(d, config))
                        .collect();
                    if !directive_vals.is_empty() {
                        map.insert("directives", json!(directive_vals));
                    }
                }
            }
            map_val
        }
        Selection::FragmentSpread(f) => {
            let mut map_val = json!({});
            {
                let map = map_val.as_object_mut().unwrap();
                map.insert("kind", json!("FragmentSpread"));
                map.insert("name", convert_name(f.fragment_name.as_str()));
                if config.emit_ast_directives() && !f.directives.is_empty() {
                    let directive_vals: Vec<_> = f
                        .directives
                        .iter()
                        .filter_map(|d| convert_directive(d, config))
                        .collect();
                    if !directive_vals.is_empty() {
                        map.insert("directives", json!(directive_vals));
                    }
                }
            }
            map_val
        }
    }
}

fn convert_name(name: &str) -> Value {
    json!({
        "kind": "Name",
        "value": name,
    })
}

fn convert_variable_def(vd: &ast::VariableDefinition, config: &CodegenConfig) -> Value {
    let mut map_val = json!({});
    {
        let map = map_val.as_object_mut().unwrap();
        map.insert("kind", json!("VariableDefinition"));
        map.insert("type", convert_type(&vd.ty));
        map.insert(
            "variable",
            json!({
                "kind": "Variable",
                "name": convert_name(vd.name.as_str()),
            }),
        );
        if config.emit_ast_variable_defaults() && vd.default_value.is_some() {
            map.insert(
                "defaultValue",
                json!(vd.default_value.as_ref().map(|v| convert_value(v))),
            );
        }
        if config.emit_ast_directives() && !vd.directives.is_empty() {
            let directive_vals: Vec<_> = vd
                .directives
                .iter()
                .filter_map(|d| convert_directive(d, config))
                .collect();
            if !directive_vals.is_empty() {
                map.insert("directives", json!(directive_vals));
            }
        }
    }
    map_val
}

fn convert_type(ty: &Type) -> Value {
    match ty {
        Type::Named(n) => json!({
            "kind": "NamedType",
            "name": convert_name(n.as_str()),
        }),
        Type::List(l) => json!({
            "kind": "ListType",
            "type": convert_type(l),
        }),
        Type::NonNullNamed(n) => json!({
            "kind": "NonNullType",
            "type": {
                "kind": "NamedType",
                "name": convert_name(n.as_str()),
            }
        }),
        Type::NonNullList(l) => json!({
            "kind": "NonNullType",
            "type": {
                "kind": "ListType",
                "type": convert_type(l),
            }
        }),
    }
}

fn convert_directive(d: &ast::Directive, config: &CodegenConfig) -> Option<Value> {
    if d.name.as_str() == "public" {
        return None;
    }

    let mut map_val = json!({});
    {
        let map = map_val.as_object_mut().unwrap();
        map.insert("kind", json!("Directive"));
        map.insert("name", convert_name(d.name.as_str()));

        if config.emit_ast_arguments() && !d.arguments.is_empty() {
            map.insert(
                "arguments",
                json!(
                    d.arguments
                        .iter()
                        .map(|a| convert_argument(a))
                        .collect::<Vec<_>>()
                ),
            );
        }
    }

    Some(map_val)
}

fn convert_argument(arg: &ast::Argument) -> Value {
    let mut map_val = json!({});
    {
        let map = map_val.as_object_mut().unwrap();
        map.insert("kind", json!("Argument"));
        map.insert("name", convert_name(arg.name.as_str()));
        map.insert("value", convert_value(&arg.value));
    }
    map_val
}

fn convert_value(v: &GqlValue) -> Value {
    match v {
        GqlValue::Variable(name) => {
            json!({ "kind": "Variable", "name": convert_name(name.as_str()) })
        }
        GqlValue::Int(i) => json!({ "kind": "IntValue", "value": i.to_string() }),
        GqlValue::Float(f) => json!({ "kind": "FloatValue", "value": f.to_string() }),
        GqlValue::String(s) => json!({ "kind": "StringValue", "value": s, "block": false }),
        GqlValue::Boolean(b) => json!({ "kind": "BooleanValue", "value": b }),
        GqlValue::Null => json!({ "kind": "NullValue" }),
        GqlValue::Enum(e) => json!({ "kind": "EnumValue", "value": e.as_str() }),
        GqlValue::List(l) => {
            json!({ "kind": "ListValue", "values": l.iter().map(|v| convert_value(v)).collect::<Vec<_>>() })
        }
        GqlValue::Object(o) => json!({
            "kind": "ObjectValue",
            "fields": o.iter().map(|(n, val)| json!({
                "kind": "ObjectField",
                "name": convert_name(n.as_str()),
                "value": convert_value(val),
            })).collect::<Vec<_>>(),
        }),
    }
}
