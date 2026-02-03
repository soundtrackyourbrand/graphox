use crate::document::DocumentState;
use apollo_compiler::Schema;
use tower_lsp::lsp_types::*;
use tree_sitter::Node;

impl DocumentState {
    pub fn get_signature_help(&self, position: Position, schema: &Schema) -> Option<SignatureHelp> {
        let byte_offset = self.position_to_byte(position);

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();
            let tree_len = root.end_byte();

            if byte_offset >= offset && byte_offset <= offset + tree_len {
                let local_byte = byte_offset.saturating_sub(offset);
                let node = root.descendant_for_byte_range(local_byte.saturating_sub(1), local_byte);

                if let Some(help) = self.find_signature_at_node(node, offset, local_byte, schema) {
                    return Some(help);
                }
            }
        }
        None
    }

    fn find_signature_at_node(
        &self,
        mut node: Option<Node>,
        offset: usize,
        cursor_offset: usize,
        schema: &Schema,
    ) -> Option<SignatureHelp> {
        let mut arguments_node = None;

        while let Some(current) = node {
            if current.kind() == "arguments" {
                arguments_node = Some(current);
                break;
            }
            node = current.parent();
        }

        let arguments = arguments_node?;
        let field_node = arguments.parent()?;
        if field_node.kind() != "field" {
            return None;
        }

        // 1. Get field name
        let mut field_name = None;
        let mut walker = field_node.walk();
        for child in field_node.children(&mut walker) {
            if child.kind() == "name" {
                field_name = Some(self.get_node_text(child, offset));
                break;
            }
        }
        let name = field_name?;

        // 2. Get parent type
        let parent_type = self.find_parent_type_for_node(field_node, offset, schema)?;

        // 3. Look up field in schema
        let field_def = match &parent_type {
            apollo_compiler::schema::ExtendedType::Object(obj) => obj.fields.get(name.as_str()),
            apollo_compiler::schema::ExtendedType::Interface(iface) => {
                iface.fields.get(name.as_str())
            }
            _ => None,
        }?;

        // 4. Construct signature info
        let mut parameters = Vec::new();
        let mut label = format!("{}(", name);

        for (i, arg) in field_def.arguments.iter().enumerate() {
            let arg_label = format!("{}: {}", arg.name, arg.ty);
            label.push_str(&arg_label);

            if i < field_def.arguments.len() - 1 {
                label.push_str(", ");
            }

            parameters.push(ParameterInformation {
                label: ParameterLabel::Simple(arg_label),
                documentation: arg
                    .description
                    .as_ref()
                    .map(|d| Documentation::String(d.to_string())),
            });
        }
        label.push(')');

        // 5. Find active parameter
        let active_parameter = self.find_active_parameter(arguments, cursor_offset);

        Some(SignatureHelp {
            signatures: vec![SignatureInformation {
                label,
                documentation: field_def
                    .description
                    .as_ref()
                    .map(|d| Documentation::String(d.to_string())),
                parameters: Some(parameters),
                active_parameter: None,
            }],
            active_signature: Some(0),
            active_parameter: Some(active_parameter as u32),
        })
    }

    fn find_active_parameter(&self, arguments: Node, cursor_offset: usize) -> usize {
        let mut active = 0;
        let mut walker = arguments.walk();
        for child in arguments.children(&mut walker) {
            if child.kind() == "argument" {
                if cursor_offset > child.end_byte() {
                    active += 1;
                } else if cursor_offset >= child.start_byte() {
                    break;
                }
            }
        }
        active
    }
}
