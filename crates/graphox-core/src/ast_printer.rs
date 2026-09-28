//! Prints the document AST that [`crate::apollo_ast`] builds back to GraphQL
//! text, laid out as graphql-js `print` lays it out.
//!
//! It reads the AST rather than the source because the AST is what codegen
//! ships: directives, aliases and inlined fragments are already the way the
//! configuration shapes them, so the text is the request a client sends rather
//! than the one that was written.

use sonic_rs::{JsonContainerTrait, JsonValueTrait, Value};

/// graphql-js breaks an argument or list line longer than this.
const MAX_LINE_LENGTH: usize = 80;

pub fn print_document(document: &Value) -> String {
    join(each(document, "definitions").map(print), "\n\n")
}

fn print(node: &Value) -> String {
    let kind = node.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    match kind {
        "Name" => str_field(node, "value").to_string(),
        "Variable" => format!("${}", child(node, "name")),
        "OperationDefinition" => {
            let var_defs = list(node, "variableDefinitions");
            let var_defs = if var_defs.iter().any(|v| v.contains('\n')) {
                wrap("(\n", &join(var_defs, "\n"), "\n)")
            } else {
                wrap("(", &join(var_defs, ", "), ")")
            };
            let prefix = join(
                [
                    str_field(node, "operation").to_string(),
                    join([child(node, "name"), var_defs], ""),
                    join(list(node, "directives"), " "),
                ],
                " ",
            );
            let selection_set = child(node, "selectionSet");
            // An anonymous query with nothing else to say is its selection set.
            if prefix == "query" {
                selection_set
            } else {
                format!("{prefix} {selection_set}")
            }
        }
        "VariableDefinition" => format!(
            "{}: {}{}{}",
            child(node, "variable"),
            child(node, "type"),
            wrap(" = ", &child(node, "defaultValue"), ""),
            wrap(" ", &join(list(node, "directives"), " "), ""),
        ),
        "SelectionSet" => block(list(node, "selections")),
        "Field" => {
            let prefix = format!(
                "{}{}",
                wrap("", &child(node, "alias"), ": "),
                child(node, "name")
            );
            join(
                [
                    line_and_args(&prefix, list(node, "arguments")),
                    wrap(" ", &join(list(node, "directives"), " "), ""),
                    wrap(" ", &child(node, "selectionSet"), ""),
                ],
                "",
            )
        }
        "Argument" | "ObjectField" => {
            format!("{}: {}", child(node, "name"), child(node, "value"))
        }
        "FragmentSpread" => format!(
            "...{}{}",
            child(node, "name"),
            wrap(" ", &join(list(node, "directives"), " "), "")
        ),
        "InlineFragment" => join(
            [
                "...".to_string(),
                wrap("on ", &child(node, "typeCondition"), ""),
                join(list(node, "directives"), " "),
                child(node, "selectionSet"),
            ],
            " ",
        ),
        "FragmentDefinition" => format!(
            "fragment {} on {} {}{}",
            child(node, "name"),
            child(node, "typeCondition"),
            wrap("", &join(list(node, "directives"), " "), " "),
            child(node, "selectionSet"),
        ),
        "IntValue" | "FloatValue" | "EnumValue" => str_field(node, "value").to_string(),
        "StringValue" => print_string(str_field(node, "value")),
        "BooleanValue" => {
            let value = node.get("value").and_then(|v| v.as_bool()).unwrap_or(false);
            value.to_string()
        }
        "NullValue" => "null".to_string(),
        "ListValue" => {
            let values = list(node, "values");
            let line = format!("[{}]", join(values.iter().cloned(), ", "));
            if line.len() > MAX_LINE_LENGTH {
                format!("[\n{}\n]", indent(&join(values, "\n")))
            } else {
                line
            }
        }
        "ObjectValue" => {
            let fields = list(node, "fields");
            let line = format!("{{ {} }}", join(fields.iter().cloned(), ", "));
            if line.len() > MAX_LINE_LENGTH {
                block(fields)
            } else {
                line
            }
        }
        "Directive" => format!(
            "@{}{}",
            child(node, "name"),
            wrap("(", &join(list(node, "arguments"), ", "), ")")
        ),
        "NamedType" => child(node, "name"),
        "ListType" => format!("[{}]", child(node, "type")),
        "NonNullType" => format!("{}!", child(node, "type")),
        _ => String::new(),
    }
}

fn each<'a>(node: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    node.get(key)
        .and_then(|v| v.as_array())
        .into_iter()
        .flat_map(|items| items.iter())
}

fn list(node: &Value, key: &str) -> Vec<String> {
    each(node, key).map(print).collect()
}

fn child(node: &Value, key: &str) -> String {
    node.get(key).map(print).unwrap_or_default()
}

fn str_field<'a>(node: &'a Value, key: &str) -> &'a str {
    node.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// Joins the non-empty parts, as graphql-js does: an absent piece leaves no
/// separator behind.
fn join(parts: impl IntoIterator<Item = String>, separator: &str) -> String {
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(separator)
}

fn wrap(start: &str, inner: &str, end: &str) -> String {
    if inner.is_empty() {
        String::new()
    } else {
        format!("{start}{inner}{end}")
    }
}

fn indent(text: &str) -> String {
    wrap("  ", &text.replace('\n', "\n  "), "")
}

fn block(items: Vec<String>) -> String {
    wrap("{\n", &indent(&join(items, "\n")), "\n}")
}

fn line_and_args(prefix: &str, args: Vec<String>) -> String {
    let line = format!(
        "{prefix}{}",
        wrap("(", &join(args.iter().cloned(), ", "), ")")
    );
    if line.len() > MAX_LINE_LENGTH {
        format!("{prefix}{}", wrap("(\n", &indent(&join(args, "\n")), "\n)"))
    } else {
        line
    }
}

fn print_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 || (0x7f..=0x9f).contains(&(c as u32)) => {
                out.push_str(&format!("\\u{:04X}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CodegenConfig;
    use crate::apollo_ast::serialize_operation;
    use apollo_compiler::{ExecutableDocument, Schema};

    const SCHEMA: &str = r#"
        directive @include(if: Boolean!) on FIELD | FRAGMENT_SPREAD | INLINE_FRAGMENT
        type Query {
            account(id: ID!, filter: Filter): Account
            search(term: String, first: Int, kinds: [Kind!], exact: Boolean): [Account!]!
        }
        type Account { id: ID!, name: String, zones: [SoundZone!]! }
        type SoundZone { id: ID!, name: String }
        input Filter { kind: Kind, tags: [String!] }
        enum Kind { BUSINESS, PERSONAL }
    "#;

    fn printed(source: &str, name: &str) -> String {
        let schema = Schema::parse_and_validate(SCHEMA, "schema.graphql").unwrap();
        let doc = ExecutableDocument::parse_and_validate(&schema, source, "q.graphql").unwrap();
        let fragments = doc
            .fragments
            .iter()
            .map(|(name, frag)| (std::sync::Arc::from(name.as_str()), frag.clone()))
            .collect();
        let op = doc.operations.get(Some(name)).unwrap();
        print_document(&serialize_operation(
            op,
            &fragments,
            &CodegenConfig::default(),
        ))
    }

    /// Expected strings were checked against graphql-js `print` over the same
    /// document.
    #[test]
    fn prints_an_operation_and_its_fragments_as_graphql_js_does() {
        let out = printed(
            r#"query Account($id: ID!, $withZones: Boolean! = false) {
                account(id: $id) { ...AccountFields zones @include(if: $withZones) { ...ZoneFields } }
            }
            fragment AccountFields on Account { id display: name }
            fragment ZoneFields on SoundZone { id name }"#,
            "Account",
        );
        assert_eq!(
            out,
            "query Account($id: ID!, $withZones: Boolean! = false) {\n  account(id: $id) {\n    ...AccountFields\n    zones @include(if: $withZones) {\n      ...ZoneFields\n    }\n  }\n}\n\nfragment AccountFields on Account {\n  id\n  display: name\n}\n\nfragment ZoneFields on SoundZone {\n  id\n  name\n}"
        );
    }

    #[test]
    fn prints_values_and_breaks_long_argument_lines() {
        let out = printed(
            r#"query Search {
                search(term: "say \"hi\"\n", first: 10, kinds: [BUSINESS, PERSONAL], exact: true) { id }
                account(id: "1", filter: { kind: BUSINESS, tags: null }) { ... on Account { id } }
            }"#,
            "Search",
        );
        assert_eq!(
            out,
            "query Search {\n  search(\n    term: \"say \\\"hi\\\"\\n\"\n    first: 10\n    kinds: [BUSINESS, PERSONAL]\n    exact: true\n  ) {\n    id\n  }\n  account(id: \"1\", filter: { kind: BUSINESS, tags: null }) {\n    ... on Account {\n      id\n    }\n  }\n}"
        );
    }
}
