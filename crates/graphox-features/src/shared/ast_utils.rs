use graphox_core::document::DocumentState;
use tree_sitter::Node;

pub fn find_parent_operation<'a>(node: Node<'a>) -> Option<Node<'a>> {
    let mut curr = Some(node);
    while let Some(n) = curr {
        if n.kind() == "operation_definition" {
            return Some(n);
        }
        curr = n.parent();
    }
    None
}

pub fn find_parent_fragment<'a>(node: Node<'a>) -> Option<Node<'a>> {
    let mut curr = Some(node);
    while let Some(n) = curr {
        if n.kind() == "fragment_definition" {
            return Some(n);
        }
        curr = n.parent();
    }
    None
}

pub fn get_field_name(doc: &DocumentState, field_node: Node, offset: usize) -> Option<String> {
    doc.find_child_by_kind(field_node, "name")
        .map(|n| doc.get_node_text(n, offset))
}

pub fn get_arg_name(doc: &DocumentState, arg_node: Node, offset: usize) -> Option<String> {
    doc.find_child_by_kind(arg_node, "name")
        .map(|n| doc.get_node_text(n, offset))
}

pub fn extract_operation_variables(
    doc: &DocumentState,
    op_node: Node,
    offset: usize,
) -> Vec<(String, String)> {
    let mut variables = Vec::new();
    if let Some(defs) = doc.find_child_by_kind(op_node, "variable_definitions") {
        let mut cursor = defs.walk();
        for vd in defs.children(&mut cursor) {
            if vd.kind() == "variable_definition" {
                let components = doc.extract_variable_definition_components(vd);
                let v_name = components
                    .variable
                    .map(|v| doc.get_node_text(v, offset))
                    .unwrap_or_default();
                let v_type = components
                    .type_node
                    .map(|t| doc.get_node_text(t, offset))
                    .unwrap_or_default();
                if !v_name.is_empty() {
                    variables.push((v_name, v_type));
                }
            }
        }
    }
    variables
}

pub fn find_variable_definition_node<'a>(
    doc: &DocumentState,
    parent: Node<'a>,
    name: &str,
    offset: usize,
) -> Option<Node<'a>> {
    if let Some(defs) = doc.find_child_by_kind(parent, "variable_definitions") {
        let mut def_walker = defs.walk();
        for def in defs.children(&mut def_walker) {
            if def.kind() == "variable_definition" {
                let components = doc.extract_variable_definition_components(def);
                if let Some(var_node) = components.variable
                    && doc.get_node_text(var_node, offset) == name
                {
                    return Some(var_node);
                }
            }
        }
    }
    None
}
