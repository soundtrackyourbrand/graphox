use apollo_compiler::{Schema, ast, schema};

pub fn get_field_def<'a>(
    parent_type: &'a schema::ExtendedType,
    field_name: &str,
) -> Option<&'a schema::FieldDefinition> {
    match parent_type {
        schema::ExtendedType::Object(obj) => obj.fields.get(field_name).map(|v| &***v),
        schema::ExtendedType::Interface(iface) => iface.fields.get(field_name).map(|v| &***v),
        _ => None,
    }
}

pub fn get_field_def_from_schema<'a>(
    schema: &'a Schema,
    parent_type_name: &str,
    field_name: &str,
) -> Option<&'a schema::FieldDefinition> {
    let ty = schema.types.get(parent_type_name)?;
    get_field_def(ty, field_name)
}

pub fn is_query_root(ty: &schema::ExtendedType, schema: &Schema) -> bool {
    schema
        .root_operation(ast::OperationType::Query)
        .and_then(|root_name| schema.types.get(root_name.as_str()))
        .map(|root_type| root_type.name() == ty.name())
        .unwrap_or(false)
}
