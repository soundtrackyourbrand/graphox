use apollo_compiler::{Schema, ast, schema};
use graphox_core::document::DocumentState;
use tree_sitter::Node;

use crate::shared::{ast_utils, doc_utils};

pub enum SemanticSymbol {
    Field {
        parent_type: schema::ExtendedType,
        field_def: ast::FieldDefinition,
        alias: Option<String>,
    },
    BuiltinField {
        name: String,
        parent_type: schema::ExtendedType,
    },
    Argument {
        parent_type: String,
        field_name: Option<String>,
        arg_def: ast::InputValueDefinition,
    },
    Directive {
        dir_def: ast::DirectiveDefinition,
    },
    Type(schema::ExtendedType),
    Variable {
        name: String,
        ty_text: String,
    },
    EnumValue {
        enum_name: String,
        val_def: ast::EnumValueDefinition,
    },
    InputObjectField {
        parent_type: schema::ExtendedType,
        field_def: ast::InputValueDefinition,
    },
    Literal {
        kind: String,
        expected_type: String,
    },
    DefaultValue {
        ty_text: String,
    },
    TypeExtension {
        type_name: String,
        adds_fields: Vec<String>,
        implements_interfaces: Vec<String>,
    },
    Operation {
        op_type: String,
        name: Option<String>,
        variables: Vec<(String, String)>,
        description: Option<String>,
    },
    Fragment {
        name: String,
        type_condition: String,
        description: Option<String>,
    },
    LocalSymbol {
        name: String,
        description: String,
    },
}

pub fn resolve_symbol_at_node(
    doc: &DocumentState,
    node: Node,
    offset: usize,
    cursor_offset: usize,
    schema: &Schema,
) -> Option<SemanticSymbol> {
    let kind = node.kind();

    // 1. Variable resolution
    if kind == "variable"
        || (kind == "name" && node.parent().is_some_and(|p| p.kind() == "variable"))
    {
        let var_node = if kind == "variable" {
            node
        } else {
            node.parent().unwrap()
        };
        let var_name = doc.get_node_text(var_node, offset);

        let mut curr = var_node;
        while let Some(parent) = curr.parent() {
            if parent.kind() == "operation_definition" {
                let variables = ast_utils::extract_operation_variables(doc, parent, offset);
                if let Some((_, ty_text)) = variables.iter().find(|(name, _)| name == &var_name) {
                    return Some(SemanticSymbol::Variable {
                        name: var_name,
                        ty_text: ty_text.clone(),
                    });
                }
            }
            curr = parent;
        }
    }

    // 2. Type Extension resolution (MUST check before schema type check)
    if kind == "name" {
        let mut curr = node;
        while let Some(parent) = curr.parent() {
            match parent.kind() {
                "object_type_extension"
                | "interface_type_extension"
                | "enum_type_extension"
                | "scalar_type_extension"
                | "union_type_extension"
                | "input_object_type_extension" => {
                    let type_name = doc.get_node_text(node, offset);
                    let mut adds_fields = Vec::new();
                    let mut implements_interfaces = Vec::new();

                    let mut cursor = parent.walk();
                    for child in parent.children(&mut cursor) {
                        match child.kind() {
                            "implements_interfaces" => {
                                let mut i_cursor = child.walk();
                                for i_child in child.children(&mut i_cursor) {
                                    if i_child.kind() == "named_type" {
                                        implements_interfaces
                                            .push(doc.get_node_text(i_child, offset));
                                    }
                                }
                            }
                            "field_definitions" | "fields_definition" => {
                                let mut fd_cursor = child.walk();
                                for fd in child.children(&mut fd_cursor) {
                                    if fd.kind() == "field_definition" {
                                        let f_name = doc
                                            .find_child_by_kind(fd, "name")
                                            .map(|n| doc.get_node_text(n, offset))
                                            .unwrap_or_default();
                                        let f_type = doc
                                            .find_child_by_kind(fd, "type")
                                            .map(|n| doc.get_node_text(n, offset))
                                            .unwrap_or_default();
                                        if !f_name.is_empty() {
                                            adds_fields.push(format!("{}: {}", f_name, f_type));
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }

                    return Some(SemanticSymbol::TypeExtension {
                        type_name,
                        adds_fields,
                        implements_interfaces,
                    });
                }
                _ => {}
            }
            curr = parent;
        }
    }

    // 3. Type resolution (Schema types) - AFTER extension check
    if kind == "name" {
        let symbol_name = doc.get_node_text(node, offset);
        if let Some(ty) = schema.types.get(symbol_name.as_str()) {
            return Some(SemanticSymbol::Type(ty.clone()));
        }

        // Local description fallback
        if let Some(description) = doc_utils::find_description(doc, &symbol_name) {
            return Some(SemanticSymbol::LocalSymbol {
                name: symbol_name,
                description,
            });
        }
    }

    // 4. Field/Argument/Directive resolution
    let mut curr = Some(node);
    while let Some(current_node) = curr {
        match current_node.kind() {
            "field" | "field_definition" => {
                let (components, is_definition) = if current_node.kind() == "field_definition" {
                    (doc.extract_field_definition_components(current_node), true)
                } else {
                    (doc.extract_field_components(current_node), false)
                };

                if let Some(name_node) = components.name {
                    let name_range =
                        (name_node.start_byte() + offset)..(name_node.end_byte() + offset);
                    if cursor_offset >= name_range.start && cursor_offset <= name_range.end {
                        let field_name = doc.get_node_text(name_node, offset);

                        let parent_type = if is_definition {
                            // Find the containing type for this definition
                            let parent_node = doc.find_ancestor_by_kinds(
                                current_node,
                                &["object_type_definition", "interface_type_definition"],
                            )?;
                            let name_node = doc.find_child_by_kind(parent_node, "name")?;
                            let name = doc.get_node_text(name_node, offset);
                            schema.types.get(name.as_str()).cloned()?
                        } else {
                            doc.find_parent_type_for_node(current_node, offset, schema)?
                        };

                        // Check for built-in fields
                        if !is_definition
                            && (field_name == "__typename"
                                || field_name == "__schema"
                                || field_name == "__type")
                        {
                            return Some(SemanticSymbol::BuiltinField {
                                name: field_name,
                                parent_type,
                            });
                        }

                        let field_def = match &parent_type {
                            schema::ExtendedType::Object(obj) => {
                                obj.fields.get(field_name.as_str()).map(|c| &c.node)
                            }
                            schema::ExtendedType::Interface(iface) => {
                                iface.fields.get(field_name.as_str()).map(|c| &**c)
                            }
                            _ => None,
                        }?;
                        let alias = components.alias.map(|a| doc.get_node_text(a, offset));

                        return Some(SemanticSymbol::Field {
                            parent_type: parent_type.clone(),
                            field_def: (**field_def).clone(),
                            alias,
                        });
                    }
                }

                // If we are in alias (only for usage)
                if !is_definition && let Some(alias_node) = components.alias {
                    let alias_range =
                        (alias_node.start_byte() + offset)..(alias_node.end_byte() + offset);
                    if cursor_offset >= alias_range.start && cursor_offset <= alias_range.end {
                        let parent_type =
                            doc.find_parent_type_for_node(current_node, offset, schema)?;
                        let name_node = components.name?;
                        let field_name = doc.get_node_text(name_node, offset);
                        let field_def = match &parent_type {
                            schema::ExtendedType::Object(obj) => {
                                obj.fields.get(field_name.as_str()).map(|c| &c.node)
                            }
                            schema::ExtendedType::Interface(iface) => {
                                iface.fields.get(field_name.as_str()).map(|c| &c.node)
                            }
                            _ => None,
                        }?;
                        let alias = Some(doc.get_node_text(alias_node, offset));

                        return Some(SemanticSymbol::Field {
                            parent_type: parent_type.clone(),
                            field_def: (**field_def).clone(),
                            alias,
                        });
                    }
                }
            }
            "argument" => {
                let name_node = doc.find_child_by_kind(current_node, "name")?;
                let name_range = (name_node.start_byte() + offset)..(name_node.end_byte() + offset);
                if cursor_offset >= name_range.start && cursor_offset <= name_range.end {
                    let arg_name = doc.get_node_text(name_node, offset);
                    let parent = current_node.parent()?;
                    let target_node = if parent.kind() == "arguments" {
                        parent.parent()
                    } else {
                        Some(parent)
                    }?;

                    if target_node.kind() == "field" {
                        let parent_type =
                            doc.find_parent_type_for_node(target_node, offset, schema)?;
                        let field_name_node = doc.extract_field_components(target_node).name?;
                        let field_name = doc.get_node_text(field_name_node, offset);
                        let field_def = match &parent_type {
                            schema::ExtendedType::Object(obj) => {
                                obj.fields.get(field_name.as_str()).map(|c| &c.node)
                            }
                            schema::ExtendedType::Interface(iface) => {
                                iface.fields.get(field_name.as_str()).map(|c| &c.node)
                            }
                            _ => None,
                        }?;
                        let arg_def = field_def
                            .arguments
                            .iter()
                            .find(|a| a.name.as_str() == arg_name)?;
                        return Some(SemanticSymbol::Argument {
                            parent_type: parent_type.name().to_string(),
                            field_name: Some(field_name),
                            arg_def: (**arg_def).clone(),
                        });
                    } else if target_node.kind() == "directive" {
                        let dir_name_node = doc.find_child_by_kind(target_node, "name")?;
                        let dir_name = doc.get_node_text(dir_name_node, offset);
                        let dir_def = schema.directive_definitions.get(dir_name.as_str())?;
                        let arg_def = dir_def
                            .arguments
                            .iter()
                            .find(|a| a.name.as_str() == arg_name)?;
                        return Some(SemanticSymbol::Argument {
                            parent_type: dir_name,
                            field_name: None,
                            arg_def: (**arg_def).clone(),
                        });
                    }
                }
            }
            "directive" | "directive_definition" => {
                let name_node = doc.find_child_by_kind(current_node, "name")?;
                let name_range = (name_node.start_byte() + offset)..(name_node.end_byte() + offset);
                if cursor_offset >= name_range.start && cursor_offset <= name_range.end {
                    let dir_name = doc.get_node_text(name_node, offset);
                    let dir_def = schema.directive_definitions.get(dir_name.as_str())?;
                    return Some(SemanticSymbol::Directive {
                        dir_def: (**dir_def).clone(),
                    });
                }
            }
            "enum_value" => {
                let val_name = doc.get_node_text(current_node, offset);
                let (root_type, path) = resolve_input_context(doc, current_node, offset, schema)?;
                let enum_type_name = resolve_type_from_path(schema, root_type, &path)?;
                if let Some(schema::ExtendedType::Enum(enm)) =
                    schema.types.get(enum_type_name.as_str())
                {
                    let val_def = enm.values.get(val_name.as_str())?;
                    return Some(SemanticSymbol::EnumValue {
                        enum_name: enum_type_name,
                        val_def: val_def.as_ref().clone(),
                    });
                }
            }
            "object_field" => {
                let name_node = doc.find_child_by_kind(current_node, "name")?;
                let name_range = (name_node.start_byte() + offset)..(name_node.end_byte() + offset);
                if cursor_offset >= name_range.start && cursor_offset <= name_range.end {
                    let field_name = doc.get_node_text(name_node, offset);
                    let object_value_node = current_node.parent()?;

                    let mut ctx_node = object_value_node;
                    while let Some(parent) = ctx_node.parent() {
                        match parent.kind() {
                            "argument"
                            | "arguments"
                            | "variable_definition"
                            | "variable_definitions" => {
                                if let Some(parent_input_type) =
                                    find_expected_type_for_node(doc, parent, offset, None, schema)
                                    && let schema::ExtendedType::InputObject(input_obj) =
                                        parent_input_type.clone()
                                    && let Some(field_def) =
                                        input_obj.fields.get(field_name.as_str())
                                {
                                    return Some(SemanticSymbol::InputObjectField {
                                        parent_type: parent_input_type,
                                        field_def: (*field_def.node).clone(),
                                    });
                                }
                                return None;
                            }
                            _ => {}
                        }
                        ctx_node = parent;
                    }
                    return None;
                }
            }
            _ => {}
        }
        curr = current_node.parent();
    }

    // 5. Operation/Fragment resolution
    let mut curr = Some(node);
    while let Some(current_node) = curr {
        match current_node.kind() {
            "operation_definition" => {
                if let Some(name_node) = doc.find_child_by_kind(current_node, "name") {
                    let name_range =
                        (name_node.start_byte() + offset)..(name_node.end_byte() + offset);

                    // Also resolve if on the operation keyword (query/mutation/subscription)
                    let is_on_keyword = current_node.child(0).is_some_and(|k| {
                        let r = (k.start_byte() + offset)..(k.end_byte() + offset);
                        cursor_offset >= r.start && cursor_offset <= r.end
                    });

                    if (cursor_offset >= name_range.start && cursor_offset <= name_range.end)
                        || is_on_keyword
                    {
                        let op_name = doc.get_node_text(name_node, offset);
                        let op_type = doc.get_operation_type(current_node, offset);
                        let variables =
                            ast_utils::extract_operation_variables(doc, current_node, offset);
                        let description = doc_utils::find_description(doc, &op_name);
                        return Some(SemanticSymbol::Operation {
                            op_type,
                            name: Some(op_name),
                            variables,
                            description,
                        });
                    }
                } else {
                    // Anonymous operation
                    let op_type_node = doc.find_child_by_kind(current_node, "operation_type");
                    if let Some(ot_node) = op_type_node {
                        let range = (ot_node.start_byte() + offset)..(ot_node.end_byte() + offset);
                        if cursor_offset >= range.start && cursor_offset <= range.end {
                            let op_type = doc.get_node_text(ot_node, offset);
                            let variables =
                                ast_utils::extract_operation_variables(doc, current_node, offset);
                            return Some(SemanticSymbol::Operation {
                                op_type,
                                name: None,
                                variables,
                                description: None,
                            });
                        }
                    }
                }
            }
            "fragment_definition" => {
                if let Some(name_node) = doc
                    .find_child_by_kind(current_node, "fragment_name")
                    .and_then(|fn_node| doc.find_child_by_kind(fn_node, "name"))
                    .or_else(|| doc.find_child_by_kind(current_node, "name"))
                {
                    let name_range =
                        (name_node.start_byte() + offset)..(name_node.end_byte() + offset);

                    // Also resolve if on the "fragment" keyword (helpful for cross-file navigation fallback)
                    let is_on_keyword = current_node.child(0).is_some_and(|k| {
                        let r = (k.start_byte() + offset)..(k.end_byte() + offset);
                        cursor_offset >= r.start && cursor_offset <= r.end
                    });

                    if (cursor_offset >= name_range.start && cursor_offset <= name_range.end)
                        || is_on_keyword
                    {
                        let frag_name = doc.get_node_text(name_node, offset);
                        let type_condition = doc
                            .get_fragment_type_condition(current_node, offset)
                            .unwrap_or_default();
                        let description = doc_utils::find_description(doc, &frag_name);
                        return Some(SemanticSymbol::Fragment {
                            name: frag_name,
                            type_condition,
                            description,
                        });
                    }
                }
            }
            _ => {}
        }
        curr = current_node.parent();
    }

    // Default value check (MUST check before literal check)

    if kind == "string_value"
        || kind == "int_value"
        || kind == "float_value"
        || kind == "boolean_value"
        || kind == "null_value"
    {
        let mut curr = Some(node);
        while let Some(parent) = curr {
            if parent.kind() == "variable_definition" {
                let components = doc.extract_variable_definition_components(parent);
                if let Some(dv_node) = components.default_value {
                    let range = (dv_node.start_byte() + offset)..(dv_node.end_byte() + offset);
                    if cursor_offset >= range.start
                        && cursor_offset <= range.end
                        && let Some(type_node) = components.type_node
                    {
                        let ty_text = doc.get_node_text(type_node, offset);
                        return Some(SemanticSymbol::DefaultValue { ty_text });
                    }
                }
            }
            curr = parent.parent();
        }
    }

    // 6. Literals (AFTER default value check)
    if (kind == "string_value"
        || kind == "int_value"
        || kind == "float_value"
        || kind == "boolean_value"
        || kind == "null_value")
        && let Some(ty) =
            find_expected_ast_type_for_node(doc, node, offset, Some(cursor_offset), schema)
    {
        return Some(SemanticSymbol::Literal {
            kind: kind.to_string(),
            expected_type: ty.to_string(),
        });
    }

    None
}

pub fn resolve_fragment_spread_at_node(
    doc: &DocumentState,
    node: Node,
    offset: usize,
    cursor_offset: usize,
) -> Option<SemanticSymbol> {
    let mut curr = Some(node);
    while let Some(current_node) = curr {
        if current_node.kind() == "fragment_spread" {
            let is_on_ellipsis = current_node.child(0).is_some_and(|k| {
                let r = (k.start_byte() + offset)..(k.end_byte() + offset);
                cursor_offset >= r.start && cursor_offset <= r.end
            });
            if let Some(name_node) = doc
                .find_child_by_kind(current_node, "fragment_name")
                .and_then(|fn_node| doc.find_child_by_kind(fn_node, "name"))
            {
                let name_range = (name_node.start_byte() + offset)..(name_node.end_byte() + offset);
                if (cursor_offset >= name_range.start && cursor_offset <= name_range.end)
                    || is_on_ellipsis
                {
                    let frag_name = doc.get_node_text(name_node, offset);
                    return Some(SemanticSymbol::Fragment {
                        name: frag_name,
                        type_condition: String::new(),
                        description: None,
                    });
                }
            }
        }
        curr = current_node.parent();
    }
    None
}

pub fn parse_type_string(text: &str) -> Option<ast::Type> {
    ast::Type::parse(text, "type.graphql").ok()
}

pub fn find_expected_type_for_node(
    doc: &DocumentState,
    node: Node,
    offset: usize,
    cursor_offset: Option<usize>,
    schema: &Schema,
) -> Option<schema::ExtendedType> {
    let ast_type = find_expected_ast_type_for_node(doc, node, offset, cursor_offset, schema)?;
    schema
        .types
        .get(ast_type.inner_named_type().as_str())
        .cloned()
}

pub fn find_expected_ast_type_for_node(
    doc: &DocumentState,
    node: Node,
    offset: usize,
    cursor_offset: Option<usize>,
    schema: &Schema,
) -> Option<ast::Type> {
    let mut curr = Some(node);
    while let Some(current_node) = curr {
        match current_node.kind() {
            "variable_definition" | "input_value_definition" => {
                let mut vd_cursor = current_node.walk();
                let mut var_type_text = None;
                for vd_child in current_node.children(&mut vd_cursor) {
                    if vd_child.kind() == "type" {
                        var_type_text = Some(doc.get_node_text(vd_child, offset));
                        break;
                    }
                }
                if let Some(text) = var_type_text {
                    return Some(parse_type_string(&text));
                }
            }
            "variable_definitions" | "arguments_definition" => {
                if let Some(co) = cursor_offset {
                    let mut cursor = current_node.walk();
                    let mut last_def = None;
                    let target_kind = if current_node.kind() == "variable_definitions" {
                        "variable_definition"
                    } else {
                        "input_value_definition"
                    };

                    for child in current_node.children(&mut cursor) {
                        if child.kind() == target_kind && child.start_byte() + offset < co {
                            last_def = Some(child);
                        }
                    }

                    if let Some(vd) = last_def {
                        let mut vd_cursor = vd.walk();
                        let mut var_type_text = None;
                        for vd_child in vd.children(&mut vd_cursor) {
                            if vd_child.kind() == "type" {
                                var_type_text = Some(doc.get_node_text(vd_child, offset));
                                break;
                            }
                        }
                        if let Some(text) = var_type_text {
                            return Some(parse_type_string(&text));
                        }
                    }
                }
            }
            "argument" | "arguments" => {
                let arg_name = if current_node.kind() == "argument" {
                    ast_utils::get_arg_name(doc, current_node, offset)
                } else if let Some(co) = cursor_offset {
                    // In arguments node, find which argument we are in or after
                    let mut cursor = current_node.walk();
                    let mut last_arg = None;
                    for child in current_node.children(&mut cursor) {
                        if child.kind() == "argument" && child.start_byte() + offset < co {
                            last_arg = Some(child);
                        }
                    }

                    if let Some(arg) = last_arg {
                        // Check if we are at value position of this argument
                        let text = doc.get_node_text(arg, offset);
                        if let Some(colon_idx) = text.find(':') {
                            let absolute_colon = arg.start_byte() + offset + colon_idx;
                            if co > absolute_colon {
                                ast_utils::get_arg_name(doc, arg, offset)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        // If no argument node found, we might be right after a name that isn't yet an argument
                        let text_before = doc
                            .rope
                            .byte_slice(current_node.start_byte() + offset..co)
                            .to_string();
                        if let Some(colon_idx) = text_before.rfind(':') {
                            let name_part = &text_before[..colon_idx];
                            name_part
                                .split(|c: char| !c.is_alphanumeric() && c != '_')
                                .rfind(|s| !s.is_empty())
                                .map(|s| s.to_string())
                        } else {
                            None
                        }
                    }
                } else {
                    None
                };

                if let Some(arg_name) = arg_name {
                    let context_node = if current_node.kind() == "arguments" {
                        current_node
                    } else {
                        current_node.parent()?
                    };
                    let target_node = context_node.parent()?; // field or directive

                    if target_node.kind() == "field" {
                        let parent_type =
                            doc.find_parent_type_for_node(target_node, offset, schema)?;
                        let field_name_node = doc.extract_field_components(target_node).name?;
                        let field_name = doc.get_node_text(field_name_node, offset);

                        let field_def = match &parent_type {
                            schema::ExtendedType::Object(obj) => {
                                obj.fields.get(field_name.as_str())
                            }
                            schema::ExtendedType::Interface(iface) => {
                                iface.fields.get(field_name.as_str())
                            }
                            _ => None,
                        }?;

                        let arg_def = field_def
                            .arguments
                            .iter()
                            .find(|a| a.name.as_str() == arg_name)?;
                        return Some((*arg_def.ty).clone());
                    } else if target_node.kind() == "directive" {
                        let name_node = doc.find_child_by_kind(target_node, "name")?;
                        let dir_name = doc.get_node_text(name_node, offset);
                        let dir_def = schema.directive_definitions.get(dir_name.as_str())?;
                        let arg_def = dir_def
                            .arguments
                            .iter()
                            .find(|a| a.name.as_str() == arg_name)?;
                        return Some((*arg_def.ty).clone());
                    }
                }
            }
            "object_field" | "object_value" => {
                let field_node = if current_node.kind() == "object_field" {
                    Some(current_node)
                } else if let Some(co) = cursor_offset {
                    // In object_value node, find which field we are in or after
                    let mut cursor = current_node.walk();
                    let mut last_f = None;
                    for child in current_node.children(&mut cursor) {
                        if child.kind() == "object_field" && child.start_byte() + offset < co {
                            last_f = Some(child);
                        }
                    }
                    last_f
                } else {
                    None
                };

                if let Some(f) = field_node {
                    // If we have an offset, check if we are actually at the value position
                    if let Some(co) = cursor_offset {
                        let text = doc.get_node_text(f, offset);
                        if let Some(colon_idx) = text.find(':') {
                            let absolute_colon = f.start_byte() + offset + colon_idx;
                            if co <= absolute_colon {
                                // We are still at the name part of the field
                                return None;
                            }
                        } else {
                            // No colon found in the field node yet
                            return None;
                        }
                    }

                    let field_name = ast_utils::get_field_name(doc, f, offset)?;

                    let object_value_node = f.parent()?;
                    let mut ctx_node = object_value_node;

                    while let Some(parent) = ctx_node.parent() {
                        match parent.kind() {
                            "argument"
                            | "arguments"
                            | "variable_definition"
                            | "variable_definitions" => {
                                if let Some(parent_input_type) =
                                    find_expected_type_for_node(doc, parent, offset, None, schema)
                                    && let schema::ExtendedType::InputObject(input_obj) =
                                        parent_input_type
                                {
                                    let field_def = input_obj.fields.get(field_name.as_str())?;
                                    return Some((*field_def.ty).clone());
                                }
                                return None;
                            }
                            _ => {}
                        }
                        ctx_node = parent;
                    }
                    return None;
                }
            }
            "list_value" => {
                // Recurse to find the type of the list itself
                let list_type = find_expected_ast_type_for_node(
                    doc,
                    current_node,
                    offset,
                    cursor_offset,
                    schema,
                )?;
                return match list_type {
                    ast::Type::List(inner) => Some(*inner),
                    ast::Type::NonNullList(inner) => Some(*inner),
                    _ => Some(list_type),
                };
            }
            _ => {}
        }
        curr = current_node.parent();
    }
    None
}

pub fn resolve_input_context(
    doc: &DocumentState,
    node: Node,
    offset: usize,
    schema: &Schema,
) -> Option<(String, Vec<String>)> {
    let mut curr = Some(node);
    let mut field_path = Vec::new();

    while let Some(current_node) = curr {
        if current_node.kind() == "object_field" {
            if let Some(field_name) = ast_utils::get_field_name(doc, current_node, offset) {
                field_path.push(field_name);
            }
        } else if current_node.kind() == "argument" {
            let arg_name = ast_utils::get_arg_name(doc, current_node, offset)?;

            let parent = current_node.parent()?;
            let target_node = if parent.kind() == "arguments" {
                parent.parent()
            } else {
                Some(parent)
            }?;

            let target_name = doc
                .find_child_by_kind(target_node, "name")
                .map(|n| doc.get_node_text(n, offset))?;

            let arg_type_name = if target_node.kind() == "field" {
                let parent_type = doc.find_parent_type_for_node(target_node, offset, schema)?;
                let field_def =
                    crate::shared::schema_utils::get_field_def(&parent_type, target_name.as_str())?;
                field_def
                    .arguments
                    .iter()
                    .find(|a| a.name.as_str() == arg_name)?
                    .ty
                    .inner_named_type()
                    .to_string()
            } else if target_node.kind() == "directive" {
                schema
                    .directive_definitions
                    .get(target_name.as_str())?
                    .arguments
                    .iter()
                    .find(|a| a.name.as_str() == arg_name)?
                    .ty
                    .inner_named_type()
                    .to_string()
            } else {
                return None;
            };

            field_path.reverse();
            return Some((arg_type_name, field_path));
        } else if current_node.kind() == "variable_definition" {
            let type_node = doc.find_child_by_kind(current_node, "type")?;
            let mut type_name = doc.get_node_text(type_node, offset);
            type_name = type_name.replace(['!', '[', ']'], "");

            field_path.reverse();
            return Some((type_name, field_path));
        }
        curr = current_node.parent();
    }
    None
}

pub fn resolve_type_from_path(
    schema: &Schema,
    mut current_type_name: String,
    path: &[String],
) -> Option<String> {
    for segment in path {
        if let Some(schema::ExtendedType::InputObject(io)) =
            schema.types.get(current_type_name.as_str())
        {
            if let Some(f) = io.fields.get(segment.as_str()) {
                current_type_name = f.ty.inner_named_type().to_string();
            } else {
                return None;
            }
        } else {
            return None;
        }
    }
    Some(current_type_name)
}
