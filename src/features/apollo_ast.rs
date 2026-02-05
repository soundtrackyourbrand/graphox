use apollo_compiler::Node;
use apollo_compiler::ast::{self, OperationType, Type, Value as GqlValue};
use apollo_compiler::executable::{self, Selection};
use fnv::{FnvHashMap as HashMap, FnvHashSet as HashSet};
use serde_json::{Value, json};

pub fn serialize_operation(
    operation: &executable::Operation,
    fragments: &HashMap<String, Node<executable::Fragment>>,
) -> Value {
    let mut definitions = Vec::new();

    // 1. Add the operation itself
    definitions.push(convert_operation(operation));

    // 2. Add all transitive fragments
    let mut used_fragments = HashSet::default();
    collect_fragments(&operation.selection_set, fragments, &mut used_fragments);

    let mut sorted_fragments: Vec<_> = used_fragments.into_iter().collect();
    sorted_fragments.sort();

    for frag_name in sorted_fragments {
        if let Some(frag) = fragments.get(&frag_name) {
            definitions.push(convert_fragment(frag));
        }
    }

    json!({
        "kind": "Document",
        "definitions": definitions,
    })
}

pub fn serialize_operation_definition(operation: &executable::Operation) -> Value {
    convert_operation(operation)
}

pub fn serialize_fragment_definition(fragment: &executable::Fragment) -> Value {
    convert_fragment(fragment)
}

pub fn get_operation_fragment_dependencies(
    operation: &executable::Operation,
    all_fragments: &HashMap<String, Node<executable::Fragment>>,
) -> HashSet<String> {
    let mut used = HashSet::default();
    collect_fragments(&operation.selection_set, all_fragments, &mut used);
    used
}

pub fn get_fragment_fragment_dependencies(
    fragment: &executable::Fragment,
    all_fragments: &HashMap<String, Node<executable::Fragment>>,
) -> HashSet<String> {
    let mut used = HashSet::default();
    collect_fragments(&fragment.selection_set, all_fragments, &mut used);
    used
}

fn collect_fragments(
    selection_set: &executable::SelectionSet,
    all_fragments: &HashMap<String, Node<executable::Fragment>>,
    used: &mut HashSet<String>,
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
                let name = spread.fragment_name.as_str().to_string();
                if used.insert(name.clone())
                    && let Some(frag) = all_fragments.get(&name) {
                        collect_fragments(&frag.selection_set, all_fragments, used);
                    }
            }
        }
    }
}

fn convert_operation(op: &executable::Operation) -> Value {
    json!({
        "kind": "OperationDefinition",
        "operation": match op.operation_type {
            OperationType::Query => "query",
            OperationType::Mutation => "mutation",
            OperationType::Subscription => "subscription",
        },
        "name": op.name.as_ref().map(|n| convert_name(n.as_str())),
        "variableDefinitions": op.variables.iter().map(|v| convert_variable_def(v)).collect::<Vec<_>>(),
        "directives": op.directives.iter().filter(|d| d.name.as_str() != "public").map(|d| convert_directive(d)).collect::<Vec<_>>(),
        "selectionSet": convert_selection_set(&op.selection_set),
    })
}

fn convert_fragment(frag: &executable::Fragment) -> Value {
    json!({
        "kind": "FragmentDefinition",
        "name": convert_name(frag.name.as_str()),
        "typeCondition": {
            "kind": "NamedType",
            "name": convert_name(frag.type_condition().as_str()),
        },
        "directives": frag.directives.iter().filter(|d| d.name.as_str() != "public").map(|d| convert_directive(d)).collect::<Vec<_>>(),
        "selectionSet": convert_selection_set(&frag.selection_set),
    })
}

fn convert_selection_set(sel: &executable::SelectionSet) -> Value {
    json!({
        "kind": "SelectionSet",
        "selections": sel.selections.iter().map(convert_selection).collect::<Vec<_>>(),
    })
}

fn convert_selection(sel: &Selection) -> Value {
    match sel {
        Selection::Field(f) => json!({
            "kind": "Field",
            "alias": f.alias.as_ref().map(|a| convert_name(a.as_str())),
            "name": convert_name(f.name.as_str()),
            "arguments": f.arguments.iter().map(|a| convert_argument(a)).collect::<Vec<_>>(),
            "directives": f.directives.iter().filter(|d| d.name.as_str() != "public").map(|d| convert_directive(d)).collect::<Vec<_>>(),
            "selectionSet": if f.selection_set.selections.is_empty() { Value::Null } else { convert_selection_set(&f.selection_set) },
        }),
        Selection::InlineFragment(f) => json!({
            "kind": "InlineFragment",
            "typeCondition": f.type_condition.as_ref().map(|t| json!({
                "kind": "NamedType",
                "name": convert_name(t.as_str()),
            })),
            "directives": f.directives.iter().filter(|d| d.name.as_str() != "public").map(|d| convert_directive(d)).collect::<Vec<_>>(),
            "selectionSet": convert_selection_set(&f.selection_set),
        }),
        Selection::FragmentSpread(f) => json!({
            "kind": "FragmentSpread",
            "name": convert_name(f.fragment_name.as_str()),
            "directives": f.directives.iter().filter(|d| d.name.as_str() != "public").map(|d| convert_directive(d)).collect::<Vec<_>>(),
        }),
    }
}

fn convert_name(name: &str) -> Value {
    json!({
        "kind": "Name",
        "value": name,
    })
}

fn convert_variable_def(vd: &ast::VariableDefinition) -> Value {
    json!({
        "kind": "VariableDefinition",
        "variable": {
            "kind": "Variable",
            "name": convert_name(vd.name.as_str()),
        },
        "type": convert_type(&vd.ty),
        "defaultValue": vd.default_value.as_ref().map(|v| convert_value(v)),
        "directives": vd.directives.iter().filter(|d| d.name.as_str() != "public").map(|d| convert_directive(d)).collect::<Vec<_>>(),
    })
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

fn convert_directive(d: &ast::Directive) -> Value {
    json!({
        "kind": "Directive",
        "name": convert_name(d.name.as_str()),
        "arguments": d.arguments.iter().map(|a| convert_argument(a)).collect::<Vec<_>>(),
    })
}

fn convert_argument(arg: &ast::Argument) -> Value {
    json!({
        "kind": "Argument",
        "name": convert_name(arg.name.as_str()),
        "value": convert_value(&arg.value),
    })
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
