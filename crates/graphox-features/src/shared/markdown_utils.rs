use apollo_compiler::{Schema, schema};
use std::sync::Arc;

pub fn schema_field_strings<'a>(
    parent_type: &'a schema::ExtendedType,
    field_name: &str,
    schema: &'a Schema,
) -> Option<(&'a schema::FieldDefinition, String, Option<String>)> {
    let candidate = match parent_type {
        schema::ExtendedType::Object(obj) => obj.fields.get(field_name).map(|v| &***v),
        schema::ExtendedType::Interface(iface) => iface.fields.get(field_name).map(|v| &***v),
        _ => schema
            .types
            .get(parent_type.name().as_str())
            .and_then(|ty| match ty {
                schema::ExtendedType::Object(obj) => obj.fields.get(field_name).map(|v| &***v),
                schema::ExtendedType::Interface(iface) => {
                    iface.fields.get(field_name).map(|v| &***v)
                }
                _ => None,
            }),
    }?;

    let ty = candidate.ty.to_string();
    let description = candidate
        .description
        .as_ref()
        .map(|d| d.as_ref().to_string());

    Some((candidate, ty, description))
}

fn format_args_str(arguments: &[apollo_compiler::Node<schema::InputValueDefinition>]) -> String {
    if arguments.is_empty() {
        return String::new();
    }
    let args: Vec<String> = arguments
        .iter()
        .map(|a| format!("{}: {}", a.name, a.ty))
        .collect();
    format!("({})", args.join(", "))
}

pub fn describe_field_markdown(
    parent_name: &str,
    field_name: &str,
    field_type: &str,
    description: Option<&str>,
    arguments: &[apollo_compiler::Node<schema::InputValueDefinition>],
    deprecation_reason: Option<&str>,
) -> String {
    let args_str = format_args_str(arguments);

    let mut info = format!(
        "### field `{}.{}{}`\n---\nType: `{}`\n",
        parent_name, field_name, args_str, field_type
    );
    if let Some(desc) = description
        && !desc.trim().is_empty()
    {
        info.push('\n');
        info.push_str(desc);
        info.push('\n');
    }
    if let Some(reason) = deprecation_reason {
        info.push_str(&format!("\n**Deprecated:** {}\n", reason));
    }
    info
}

pub fn describe_field_markdown_with_alias(
    parent_name: &str,
    field_name: &str,
    alias_name: &str,
    field_type: &str,
    description: Option<&str>,
    arguments: &[apollo_compiler::Node<schema::InputValueDefinition>],
    deprecation_reason: Option<&str>,
) -> String {
    let args_str = format_args_str(arguments);

    let mut info = format!(
        "### field `{}.{}{}` (aliased as `{}`)\n---\nType: `{}`\n",
        parent_name, field_name, args_str, alias_name, field_type
    );
    if let Some(desc) = description
        && !desc.trim().is_empty()
    {
        info.push('\n');
        info.push_str(desc);
        info.push('\n');
    }
    if let Some(reason) = deprecation_reason {
        info.push_str(&format!("\n**Deprecated:** {}\n", reason));
    }
    info
}

pub fn describe_local_markdown(name: &str, description: &str) -> String {
    format!("### {}\n---\n{}", name, description)
}

pub fn describe_fragment_completion_markdown<'a>(
    description: Option<&str>,
    requirements: impl Iterator<Item = (&'a Arc<str>, &'a Arc<str>)>,
    import_path: Option<&str>,
) -> String {
    let mut documentation = description.map(|s| s.to_string()).unwrap_or_default();
    let mut has_reqs = false;
    for (var, ty) in requirements {
        if !has_reqs {
            if !documentation.is_empty() {
                documentation.push_str("\n\n---\n\n");
            }
            documentation.push_str("**Requires Variables:**\n");
            has_reqs = true;
        }
        documentation.push_str(&format!("- `${}`: `{}`\n", var, ty));
    }
    if let Some(import) = import_path {
        if !documentation.is_empty() {
            documentation.push_str("\n\n---\n\n");
        }
        documentation.push_str(&format!("Import: `{}`", import));
    }
    documentation
}

pub fn describe_argument_markdown(
    arg_name: &str,
    arg_type: &str,
    description: Option<&str>,
) -> String {
    let mut info = format!("### argument {}\n---\nType: `{}`\n", arg_name, arg_type);
    if let Some(desc) = description
        && !desc.trim().is_empty()
    {
        info.push('\n');
        info.push_str(desc);
    }
    info
}

pub fn describe_directive_markdown(
    dir_name: &str,
    description: Option<&str>,
    arguments: &[apollo_compiler::Node<schema::InputValueDefinition>],
) -> String {
    let mut info = format!("### directive @{}\n---\n", dir_name);
    if !arguments.is_empty() {
        info.push_str("Args: ");
        let args: Vec<String> = arguments
            .iter()
            .map(|a| format!("{}: `{}`", a.name, a.ty))
            .collect();
        info.push_str(&args.join(", "));
        info.push_str("\n\n");
    }
    if let Some(desc) = description
        && !desc.trim().is_empty()
    {
        info.push_str(desc);
    }
    info
}

pub fn describe_fragment_markdown(
    name: &str,
    type_condition: &str,
    description: Option<&str>,
) -> String {
    let mut info = format!(
        "### fragment {}\n---\nOn type: `{}`\n",
        name, type_condition
    );
    if let Some(desc) = description
        && !desc.trim().is_empty()
    {
        info.push_str("\n---\n");
        info.push_str(desc);
    }
    info
}

pub fn describe_operation_markdown(
    op_type: &str,
    op_name: Option<&str>,
    variables: &[(String, String)],
    description: Option<&str>,
) -> String {
    let mut info = format!("### {} {}\n", op_type, op_name.unwrap_or(""));
    info.push_str("---\n");
    if !variables.is_empty() {
        info.push_str("Variables: ");
        let vars: Vec<String> = variables
            .iter()
            .map(|(name, ty)| format!("{}: `{}`", name, ty))
            .collect();
        info.push_str(&vars.join(", "));
        info.push_str("\n\n");
    }
    if let Some(desc) = description {
        info.push_str(desc);
    }
    info
}

pub fn describe_variable_markdown(var_name: &str, var_type: &str) -> String {
    format!("### variable {}\n---\nType: `{}`", var_name, var_type)
}

pub fn describe_literal_markdown(kind: &str, expected_type: &str) -> String {
    let display_kind = match kind {
        "string_value" => "string value",
        "int_value" => "int value",
        "float_value" => "float value",
        "boolean_value" => "boolean value",
        "null_value" => "null value",
        _ => "value",
    };
    format!(
        "### {}\n---\nExpected type: `{}`",
        display_kind, expected_type
    )
}

pub fn describe_enum_value_markdown(
    enum_name: &str,
    value_name: &str,
    description: Option<&str>,
    deprecation_reason: Option<&str>,
) -> String {
    let mut info = format!(
        "### enum value {}\n---\nType: `{}`\n",
        value_name, enum_name
    );
    if let Some(desc) = description
        && !desc.trim().is_empty()
    {
        info.push('\n');
        info.push_str(desc);
        info.push('\n');
    }
    if let Some(reason) = deprecation_reason {
        info.push_str(&format!("\n**Deprecated:** {}\n", reason));
    }
    info
}

pub fn describe_enum_value_completion_markdown(
    enum_name: &str,
    description: Option<&str>,
) -> String {
    let mut info = format!("Enum value of `{}`", enum_name);
    if let Some(desc) = description
        && !desc.trim().is_empty()
    {
        info.push_str("\n\n---\n\n");
        info.push_str(desc);
    }
    info
}

pub fn describe_keyword_detail(keyword: &str) -> String {
    format!("{} operation type", keyword.to_uppercase())
}

pub fn describe_schema_keyword_detail(keyword: &str) -> String {
    format!("Schema definition keyword: {}", keyword)
}

pub fn describe_type_markdown(type_name: &str, kind: &str, description: Option<&str>) -> String {
    let mut info = format!("### {} {}\n---\n", kind, type_name);
    if let Some(desc) = description
        && !desc.trim().is_empty()
    {
        info.push_str(desc);
    }
    info
}

pub fn describe_full_type_markdown(
    name: &str,
    ty: &schema::ExtendedType,
    implementations: Option<&[String]>,
) -> String {
    let mut output = String::new();

    match ty {
        schema::ExtendedType::Scalar(s) => {
            let kind = if s.is_built_in() {
                "built-in scalar"
            } else {
                "scalar"
            };
            output.push_str(&format!("### {} {}\n", kind, name));
        }
        schema::ExtendedType::Object(_) => output.push_str(&format!("### type {}\n", name)),
        schema::ExtendedType::Interface(_) => output.push_str(&format!("### interface {}\n", name)),
        schema::ExtendedType::Union(_) => output.push_str(&format!("### union {}\n", name)),
        schema::ExtendedType::Enum(_) => output.push_str(&format!("### enum {}\n", name)),
        schema::ExtendedType::InputObject(_) => output.push_str(&format!("### input {}\n", name)),
    }

    output.push_str("---\n");

    if let Some(desc) = ty.description() {
        output.push_str(desc);
        output.push_str("\n\n");
    }

    if let Some(impls) = implementations
        && !impls.is_empty()
    {
        let mut sorted_impls: Vec<String> = impls.to_vec();
        sorted_impls.sort_unstable();
        let formatted_impls: Vec<String> =
            sorted_impls.iter().map(|i| format!("`{}`", i)).collect();
        output.push_str(&format!(
            "**Implementations:** {}\n\n",
            formatted_impls.join(", ")
        ));
    }

    match ty {
        schema::ExtendedType::Object(obj) => {
            if !obj.implements_interfaces.is_empty() {
                let ifaces: Vec<String> = obj
                    .implements_interfaces
                    .iter()
                    .map(|i| format!("`{}`", i))
                    .collect();
                output.push_str(&format!("**Implements:** {}\n\n", ifaces.join(", ")));
            }
            output.push_str("#### Fields\n");
            for (field_name, field_def) in &obj.fields {
                let desc = field_def
                    .description
                    .as_ref()
                    .map(|d| format!(" — {}", d))
                    .unwrap_or_default();
                output.push_str(&format!(
                    "- **{}**: `{}`{}\n",
                    field_name, field_def.ty, desc
                ));
            }
        }
        schema::ExtendedType::Interface(iface) => {
            if !iface.implements_interfaces.is_empty() {
                let ifaces: Vec<String> = iface
                    .implements_interfaces
                    .iter()
                    .map(|i| format!("`{}`", i))
                    .collect();
                output.push_str(&format!("**Implements:** {}\n\n", ifaces.join(", ")));
            }
            output.push_str("#### Fields\n");
            for (field_name, field_def) in &iface.fields {
                let desc = field_def
                    .description
                    .as_ref()
                    .map(|d| format!(" — {}", d))
                    .unwrap_or_default();
                output.push_str(&format!(
                    "- **{}**: `{}`{}\n",
                    field_name, field_def.ty, desc
                ));
            }
        }
        schema::ExtendedType::InputObject(input) => {
            output.push_str("#### Fields\n");
            for (field_name, field_def) in &input.fields {
                let desc = field_def
                    .description
                    .as_ref()
                    .map(|d| format!(" — {}", d))
                    .unwrap_or_default();
                output.push_str(&format!(
                    "- **{}**: `{}`{}\n",
                    field_name, field_def.ty, desc
                ));
            }
        }
        schema::ExtendedType::Enum(enm) => {
            output.push_str("#### Values\n");
            for (val_name, val_def) in &enm.values {
                let desc = val_def
                    .description
                    .as_ref()
                    .map(|d| format!(" — {}", d))
                    .unwrap_or_default();
                output.push_str(&format!("- `{}`{}\n", val_name, desc));
            }
        }
        schema::ExtendedType::Union(un) => {
            output.push_str("#### Members\n");
            for member in &un.members {
                output.push_str(&format!("- `{}`\n", member));
            }
        }
        _ => {}
    }

    output
}

pub fn describe_extension_markdown(
    type_name: &str,
    adds_fields: &[String],
    implements_interfaces: &[String],
) -> String {
    let mut info = format!("### extends {}\n---\n", type_name);
    if !adds_fields.is_empty() {
        info.push_str("Adds: ");
        let fields: Vec<String> = adds_fields.iter().map(|f| format!("`{}`", f)).collect();
        info.push_str(&fields.join(", "));
        info.push('\n');
    }
    if !implements_interfaces.is_empty() {
        info.push_str("Implements: ");
        let ifaces: Vec<String> = implements_interfaces
            .iter()
            .map(|i| format!("`{}`", i))
            .collect();
        info.push_str(&ifaces.join(", "));
        info.push('\n');
    }
    info
}

pub fn describe_default_value_markdown(ty_text: &str) -> String {
    format!(
        "### default value\n---\nType: `{}`\n\nMatches variable type",
        ty_text
    )
}

pub fn describe_alias_markdown(
    alias_name: &str,
    parent_name: &str,
    field_name: &str,
    field_type: &str,
    description: Option<&str>,
) -> String {
    let mut info = format!(
        "### alias `{}` → field `{}.{}`\n---\nType: `{}`\n",
        alias_name, parent_name, field_name, field_type
    );
    if let Some(desc) = description
        && !desc.trim().is_empty()
    {
        info.push('\n');
        info.push_str(desc);
    }
    info
}

pub fn describe_builtin_field_markdown(
    name: &str,
    parent_type: &schema::ExtendedType,
    schema: &Schema,
) -> String {
    match name {
        "__typename" => {
            if let Some((field_def, field_type, description)) =
                schema_field_strings(parent_type, "__typename", schema)
            {
                return describe_field_markdown(
                    parent_type.name(),
                    "__typename",
                    field_type.as_str(),
                    description.as_deref(),
                    &field_def.arguments,
                    None,
                );
            }

            describe_field_markdown(
                parent_type.name(),
                "__typename",
                "String!",
                Some("The GraphQL type name of the current selection."),
                &[],
                None,
            )
        }
        "__schema" | "__type" => {
            let fallback_desc = if name == "__schema" {
                "Access the current schema introspection object."
            } else {
                "Look up a type definition by its name."
            };

            let fallback_type = if name == "__schema" {
                "__Schema!"
            } else {
                "__Type"
            };

            if let Some((field_def, schema_type, description)) =
                schema_field_strings(parent_type, name, schema)
            {
                return describe_field_markdown(
                    parent_type.name(),
                    name,
                    schema_type.as_str(),
                    description.as_deref(),
                    &field_def.arguments,
                    None,
                );
            }

            describe_field_markdown(
                parent_type.name(),
                name,
                fallback_type,
                Some(fallback_desc),
                &[],
                None,
            )
        }
        _ => "".to_string(),
    }
}
