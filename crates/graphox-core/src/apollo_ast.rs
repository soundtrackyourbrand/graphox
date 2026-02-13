use ahash::{AHashMap as HashMap, AHashSet as HashSet};
use apollo_compiler::Node;
use apollo_compiler::ast::{self, OperationType, Type, Value as GqlValue};
use apollo_compiler::executable::{self, Selection};
use serde_json::{Value, json};

#[derive(Debug, Clone, Default)]
pub struct AstEmitConfig {
    pub emit_directives: bool,
    pub emit_aliases: bool,
    pub emit_arguments: bool,
    pub emit_variable_defaults: bool,
    pub inline_fragments: bool,
}

impl AstEmitConfig {
    pub fn default() -> Self {
        Self {
            emit_directives: true,
            emit_aliases: true,
            emit_arguments: true,
            emit_variable_defaults: true,
            inline_fragments: false,
        }
    }

    pub fn minimal() -> Self {
        Self {
            emit_directives: false,
            emit_aliases: false,
            emit_arguments: false,
            emit_variable_defaults: false,
            inline_fragments: false,
        }
    }

    pub fn from_config(
        emit_directives: Option<bool>,
        emit_aliases: Option<bool>,
        emit_arguments: Option<bool>,
        emit_variable_defaults: Option<bool>,
        inline_fragments: Option<bool>,
    ) -> Self {
        Self {
            emit_directives: emit_directives.unwrap_or(true),
            emit_aliases: emit_aliases.unwrap_or(true),
            emit_arguments: emit_arguments.unwrap_or(true),
            emit_variable_defaults: emit_variable_defaults.unwrap_or(true),
            inline_fragments: inline_fragments.unwrap_or(false),
        }
    }
}

pub fn serialize_operation(
    operation: &executable::Operation,
    fragments: &HashMap<String, Node<executable::Fragment>>,
    config: &AstEmitConfig,
) -> Value {
    // Pre-allocate definitions vector with estimated capacity
    // Typical operation has 1 op + N fragments where N is usually small
    let mut definitions = Vec::with_capacity(16);

    // 1. Add the operation itself
    definitions.push(convert_operation(operation, fragments, config));

    // 2. Add all transitive fragments
    let mut used_fragments = HashSet::default();
    collect_fragments(&operation.selection_set, fragments, &mut used_fragments);

    // Pre-allocate sorted_fragments with known size to avoid reallocation
    let mut sorted_fragments: Vec<_> = Vec::with_capacity(used_fragments.len());
    sorted_fragments.extend(used_fragments);
    sorted_fragments.sort_unstable(); // unstable sort is faster when element order doesn't matter

    for frag_name in sorted_fragments {
        if let Some(frag) = fragments.get(&frag_name) {
            definitions.push(convert_fragment(frag, fragments, config));
        }
    }

    json!({
        "kind": "Document",
        "definitions": definitions,
    })
}

pub fn serialize_operation_definition(
    operation: &executable::Operation,
    all_fragments: &HashMap<String, Node<executable::Fragment>>,
    config: &AstEmitConfig,
) -> Value {
    convert_operation(operation, all_fragments, config)
}

pub fn serialize_fragment_definition(
    fragment: &executable::Fragment,
    all_fragments: &HashMap<String, Node<executable::Fragment>>,
    config: &AstEmitConfig,
) -> Value {
    convert_fragment(fragment, all_fragments, config)
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
                    && let Some(frag) = all_fragments.get(&name)
                {
                    collect_fragments(&frag.selection_set, all_fragments, used);
                }
            }
        }
    }
}

fn convert_operation(
    op: &executable::Operation,
    all_fragments: &HashMap<String, Node<executable::Fragment>>,
    config: &AstEmitConfig,
) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("kind".to_string(), json!("OperationDefinition"));
    map.insert(
        "operation".to_string(),
        json!(match op.operation_type {
            OperationType::Query => "query",
            OperationType::Mutation => "mutation",
            OperationType::Subscription => "subscription",
        }),
    );
    if let Some(n) = &op.name {
        map.insert("name".to_string(), convert_name(n.as_str()));
    }
    if !op.variables.is_empty() {
        map.insert(
            "variableDefinitions".to_string(),
            json!(
                op.variables
                    .iter()
                    .map(|v| convert_variable_def(v, config))
                    .collect::<Vec<_>>()
            ),
        );
    }
    if config.emit_directives && !op.directives.is_empty() {
        map.insert(
            "directives".to_string(),
            json!(
                op.directives
                    .iter()
                    .filter(|d| d.name.as_str() != "public")
                    .map(|d| convert_directive(d))
                    .collect::<Vec<_>>()
            ),
        );
    }
    map.insert(
        "selectionSet".to_string(),
        convert_selection_set(&op.selection_set, all_fragments, config),
    );
    Value::Object(map)
}

fn convert_fragment(
    frag: &executable::Fragment,
    all_fragments: &HashMap<String, Node<executable::Fragment>>,
    config: &AstEmitConfig,
) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("kind".to_string(), json!("FragmentDefinition"));
    map.insert("name".to_string(), convert_name(frag.name.as_str()));
    map.insert(
        "typeCondition".to_string(),
        json!({
            "kind": "NamedType",
            "name": convert_name(frag.type_condition().as_str()),
        }),
    );
    if config.emit_directives && !frag.directives.is_empty() {
        map.insert(
            "directives".to_string(),
            json!(
                frag.directives
                    .iter()
                    .filter(|d| d.name.as_str() != "public")
                    .map(|d| convert_directive(d))
                    .collect::<Vec<_>>()
            ),
        );
    }
    map.insert(
        "selectionSet".to_string(),
        convert_selection_set(&frag.selection_set, all_fragments, config),
    );
    Value::Object(map)
}

fn convert_selection_set(
    sel: &executable::SelectionSet,
    all_fragments: &HashMap<String, Node<executable::Fragment>>,
    config: &AstEmitConfig,
) -> Value {
    let mut selections = Vec::new();
    let mut seen_fields = HashSet::new();

    for selection in &sel.selections {
        match selection {
            Selection::Field(field) => {
                let key = field_key(field);
                if seen_fields.insert(key) {
                    selections.push(convert_selection(selection, all_fragments, config));
                }
            }
            Selection::InlineFragment(_inline) => {
                selections.push(convert_selection(selection, all_fragments, config));
            }
            Selection::FragmentSpread(spread) => {
                if config.inline_fragments {
                    let frag_name = spread.fragment_name.as_str();
                    if let Some(frag) = all_fragments.get(frag_name) {
                        for frag_selection in &frag.selection_set.selections {
                            match frag_selection {
                                Selection::Field(field) => {
                                    let key = field_key(field);
                                    if seen_fields.insert(key) {
                                        selections.push(convert_selection(
                                            frag_selection,
                                            all_fragments,
                                            config,
                                        ));
                                    }
                                }
                                Selection::InlineFragment(_inline) => {
                                    selections.push(convert_selection(
                                        frag_selection,
                                        all_fragments,
                                        config,
                                    ));
                                }
                                Selection::FragmentSpread(nested_spread) => {
                                    let nested_frag_name = nested_spread.fragment_name.as_str();
                                    if let Some(nested_frag) = all_fragments.get(nested_frag_name) {
                                        for nested_selection in
                                            &nested_frag.selection_set.selections
                                        {
                                            match nested_selection {
                                                Selection::Field(nested_field) => {
                                                    let key = field_key(nested_field);
                                                    if seen_fields.insert(key) {
                                                        selections.push(convert_selection(
                                                            nested_selection,
                                                            all_fragments,
                                                            config,
                                                        ));
                                                    }
                                                }
                                                Selection::InlineFragment(_nested_inline) => {
                                                    selections.push(convert_selection(
                                                        nested_selection,
                                                        all_fragments,
                                                        config,
                                                    ));
                                                }
                                                Selection::FragmentSpread(deep_nested) => {
                                                    expand_deep_nested_fragment(
                                                        all_fragments,
                                                        deep_nested,
                                                        &mut selections,
                                                        &mut seen_fields,
                                                        config,
                                                    );
                                                }
                                            }
                                        }
                                    }
                                }
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

fn expand_deep_nested_fragment(
    all_fragments: &HashMap<String, Node<executable::Fragment>>,
    spread: &executable::FragmentSpread,
    selections: &mut Vec<Value>,
    seen_fields: &mut HashSet<String>,
    config: &AstEmitConfig,
) {
    let frag_name = spread.fragment_name.as_str();
    if let Some(frag) = all_fragments.get(frag_name) {
        for frag_selection in &frag.selection_set.selections {
            match frag_selection {
                Selection::Field(field) => {
                    let key = field_key(field);
                    if seen_fields.insert(key) {
                        selections.push(convert_selection(frag_selection, all_fragments, config));
                    }
                }
                Selection::InlineFragment(_inline) => {
                    selections.push(convert_selection(frag_selection, all_fragments, config));
                }
                Selection::FragmentSpread(nested_spread) => {
                    expand_deep_nested_fragment(
                        all_fragments,
                        nested_spread,
                        selections,
                        seen_fields,
                        config,
                    );
                }
            }
        }
    }
}

fn field_key(field: &Node<executable::Field>) -> String {
    match &field.alias {
        Some(alias) => format!("{}:{}", alias.as_str(), field.name.as_str()),
        None => field.name.as_str().to_string(),
    }
}

fn convert_selection(
    sel: &Selection,
    all_fragments: &HashMap<String, Node<executable::Fragment>>,
    config: &AstEmitConfig,
) -> Value {
    match sel {
        Selection::Field(f) => {
            let mut map = serde_json::Map::new();
            map.insert("kind".to_string(), json!("Field"));
            map.insert("name".to_string(), convert_name(f.name.as_str()));
            if config.emit_aliases {
                if let Some(alias) = &f.alias {
                    map.insert("alias".to_string(), convert_name(alias.as_str()));
                }
            }
            if config.emit_arguments && !f.arguments.is_empty() {
                map.insert(
                    "arguments".to_string(),
                    json!(
                        f.arguments
                            .iter()
                            .map(|a| convert_argument(a))
                            .collect::<Vec<_>>()
                    ),
                );
            }
            if config.emit_directives && !f.directives.is_empty() {
                map.insert(
                    "directives".to_string(),
                    json!(
                        f.directives
                            .iter()
                            .map(|d| convert_directive(d))
                            .collect::<Vec<_>>()
                    ),
                );
            }
            if !f.selection_set.selections.is_empty() {
                map.insert(
                    "selectionSet".to_string(),
                    convert_selection_set(&f.selection_set, all_fragments, config),
                );
            }
            Value::Object(map)
        }
        Selection::InlineFragment(f) => {
            let mut map = serde_json::Map::new();
            map.insert("kind".to_string(), json!("InlineFragment"));
            if let Some(t) = &f.type_condition {
                map.insert(
                    "typeCondition".to_string(),
                    json!({
                        "kind": "NamedType",
                        "name": convert_name(t.as_str()),
                    }),
                );
            }
            map.insert(
                "selectionSet".to_string(),
                convert_selection_set(&f.selection_set, all_fragments, config),
            );
            Value::Object(map)
        }
        Selection::FragmentSpread(f) => {
            let mut map = serde_json::Map::new();
            map.insert("kind".to_string(), json!("FragmentSpread"));
            map.insert("name".to_string(), convert_name(f.fragment_name.as_str()));
            Value::Object(map)
        }
    }
}

fn convert_name(name: &str) -> Value {
    json!({
        "kind": "Name",
        "value": name,
    })
}

fn convert_variable_def(vd: &ast::VariableDefinition, config: &AstEmitConfig) -> Value {
    let mut map = serde_json::Map::new();
    map.insert("kind".to_string(), json!("VariableDefinition"));
    map.insert(
        "variable".to_string(),
        json!({
            "kind": "Variable",
            "name": convert_name(vd.name.as_str()),
        }),
    );
    map.insert("type".to_string(), convert_type(&vd.ty));
    if config.emit_variable_defaults && vd.default_value.is_some() {
        map.insert(
            "defaultValue".to_string(),
            json!(vd.default_value.as_ref().map(|v| convert_value(v))),
        );
    }
    if config.emit_directives && !vd.directives.is_empty() {
        map.insert(
            "directives".to_string(),
            json!(
                vd.directives
                    .iter()
                    .filter(|d| d.name.as_str() != "public")
                    .map(|d| convert_directive(d))
                    .collect::<Vec<_>>()
            ),
        );
    }
    Value::Object(map)
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
