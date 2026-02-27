use apollo_compiler::Schema;
use graphox_core::document::DocumentState;
use lsp_types::{CompletionItem, CompletionItemKind};
use tree_sitter::Node;

use crate::completion::fields;
use crate::completion::types::{FragmentCompletionInfo, FragmentRequirementsResolver};

pub fn get_operation_variables(
    doc: &DocumentState,
    root: Node,
    offset: usize,
    cursor_offset: usize,
) -> Vec<CompletionItem> {
    let local_byte = cursor_offset.saturating_sub(offset);
    let current = root.descendant_for_byte_range(local_byte.saturating_sub(1), local_byte);

    let target_op = current.and_then(|c| doc.find_ancestor_by_kind(c, "operation_definition"));

    if let Some(op) = target_op {
        let mut variables = Vec::new();
        let mut walker = op.walk();
        for child in op.children(&mut walker) {
            if child.kind() == "variable_definitions" {
                let mut def_walker = child.walk();
                for def in child.children(&mut def_walker) {
                    if def.kind() == "variable_definition" {
                        let mut var_walker = def.walk();
                        for var_child in def.children(&mut var_walker) {
                            if var_child.kind() == "variable" {
                                let name = doc.get_node_text(var_child, offset);
                                variables.push(CompletionItem {
                                    label: name,
                                    kind: Some(CompletionItemKind::VARIABLE),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }
            }
        }
        return variables;
    }

    Vec::new()
}

pub fn complete_operation(
    doc: &DocumentState,
    node: Node,
    offset: usize,
    cursor_offset: usize,
    schema: &Schema,
    subgraphs: Option<&[graphox_core::schema::SubgraphInfo]>,
    fragments: &[FragmentCompletionInfo],
    resolve_requirements: FragmentRequirementsResolver,
) -> Option<Vec<CompletionItem>> {
    let operation_type_string = doc.get_operation_type(node, offset);

    let op_type = match operation_type_string.as_str() {
        "query" => Some(apollo_compiler::ast::OperationType::Query),
        "mutation" => Some(apollo_compiler::ast::OperationType::Mutation),
        "subscription" => Some(apollo_compiler::ast::OperationType::Subscription),
        _ => None,
    };

    if let Some(op) = op_type
        && let Some(root_def_name) = schema.root_operation(op)
        && let Some(root_type) = schema.types.get(root_def_name.as_str())
    {
        return fields::complete_selection_set_recursive(
            doc,
            node,
            offset,
            cursor_offset,
            root_type,
            schema,
            subgraphs,
            fragments,
            resolve_requirements,
        );
    }
    None
}
