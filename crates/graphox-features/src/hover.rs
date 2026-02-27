use crate::shared::markdown_utils::*;
use crate::shared::type_resolver::{self, SemanticSymbol};
use apollo_compiler::Schema;
use graphox_core::document::DocumentState;
use lsp_types::*;

pub trait DocumentHover {
    fn get_hover_info(
        &self,
        position: Position,
        schema: &Schema,
        subgraphs: Option<&[graphox_core::schema::SubgraphInfo]>,
    ) -> Option<Hover>;
}

impl DocumentHover for DocumentState {
    fn get_hover_info(
        &self,
        position: Position,
        schema: &Schema,
        subgraphs: Option<&[graphox_core::schema::SubgraphInfo]>,
    ) -> Option<Hover> {
        let byte_offset = self.position_to_byte(position);

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();
            let tree_len = root.end_byte();

            if byte_offset >= offset && byte_offset < offset + tree_len {
                let local_byte = byte_offset - offset;
                let mut node = root.descendant_for_byte_range(local_byte, local_byte)?;

                // If we are on a symbol that's part of a larger construct, move up
                if node.kind() == "$"
                    && let Some(parent) = node.parent()
                {
                    node = parent;
                }

                if let Some(symbol) =
                    type_resolver::resolve_symbol_at_node(self, node, offset, byte_offset, schema)
                {
                    let mut markdown = match &symbol {
                        SemanticSymbol::Field {
                            parent_type,
                            field_def,
                            alias,
                        } => {
                            if let Some(alias_name) = alias {
                                describe_field_markdown_with_alias(
                                    parent_type.name(),
                                    field_def.name.as_str(),
                                    alias_name,
                                    field_def.ty.to_string().as_str(),
                                    field_def.description.as_deref(),
                                )
                            } else {
                                describe_field_markdown(
                                    parent_type.name(),
                                    field_def.name.as_str(),
                                    field_def.ty.to_string().as_str(),
                                    field_def.description.as_deref(),
                                )
                            }
                        }
                        SemanticSymbol::BuiltinField { name, parent_type } => {
                            describe_builtin_field_markdown(name, parent_type, schema)
                        }
                        SemanticSymbol::Argument { arg_def, .. } => describe_argument_markdown(
                            arg_def.name.as_str(),
                            &arg_def.ty.to_string(),
                            arg_def.description.as_deref(),
                        ),
                        SemanticSymbol::Directive { dir_def } => describe_directive_markdown(
                            dir_def.name.as_str(),
                            dir_def.description.as_deref(),
                            &dir_def.arguments,
                        ),
                        SemanticSymbol::Type(ty) => describe_full_type_markdown(ty.name(), ty),
                        SemanticSymbol::Variable { name, ty_text } => {
                            describe_variable_markdown(name, ty_text)
                        }
                        SemanticSymbol::EnumValue { enum_name, val_def } => {
                            let deprecation_reason = val_def
                                .directives
                                .iter()
                                .find(|d| d.name == "deprecated")
                                .and_then(|d| {
                                    d.argument_by_name("reason", schema)
                                        .ok()
                                        .and_then(|arg| arg.as_str())
                                });
                            describe_enum_value_markdown(
                                enum_name,
                                val_def.value.as_str(),
                                val_def.description.as_deref(),
                                deprecation_reason,
                            )
                        }
                        SemanticSymbol::InputObjectField {
                            parent_type,
                            field_def,
                        } => describe_field_markdown(
                            parent_type.name(),
                            field_def.name.as_str(),
                            field_def.ty.to_string().as_str(),
                            field_def.description.as_deref(),
                        ),
                        SemanticSymbol::Literal {
                            kind,
                            expected_type,
                        } => describe_literal_markdown(kind, expected_type),
                        SemanticSymbol::DefaultValue { ty_text } => {
                            describe_default_value_markdown(ty_text)
                        }
                        SemanticSymbol::TypeExtension {
                            type_name,
                            adds_fields,
                            implements_interfaces,
                        } => describe_extension_markdown(
                            type_name,
                            adds_fields,
                            implements_interfaces,
                        ),
                        SemanticSymbol::Operation {
                            op_type,
                            name,
                            variables,
                            description,
                        } => describe_operation_markdown(
                            op_type,
                            name.as_deref(),
                            variables,
                            description.as_deref(),
                        ),
                        SemanticSymbol::Fragment {
                            name,
                            type_condition,
                            description,
                        } => {
                            // Fragments are usually hovered to see their source or description
                            // We use describe_local_markdown for now as it fits
                            describe_local_markdown(
                                name,
                                &format!(
                                    "Fragment on `{}`\n\n{}",
                                    type_condition,
                                    description.as_deref().unwrap_or_default()
                                ),
                            )
                        }
                        SemanticSymbol::LocalSymbol { name, description } => {
                            describe_local_markdown(name, description)
                        }
                    };

                    // Add subgraph info if available
                    if let Some(subgraphs) = subgraphs {
                        match &symbol {
                            SemanticSymbol::Field {
                                parent_type,
                                field_def,
                                ..
                            } => {
                                let mut found_subgraphs = Vec::new();
                                for sg in subgraphs {
                                    if let Some(sg_ty) = sg.schema.types.get(parent_type.name()) {
                                        let has_field = match sg_ty {
                                            apollo_compiler::schema::ExtendedType::Object(obj) => {
                                                obj.fields.contains_key(field_def.name.as_str())
                                            }
                                            apollo_compiler::schema::ExtendedType::Interface(
                                                iface,
                                            ) => iface.fields.contains_key(field_def.name.as_str()),
                                            _ => false,
                                        };
                                        if has_field {
                                            let mut sg_info = sg.name.clone();
                                            if let Some(owner) = &sg.owner {
                                                sg_info.push_str(" (");
                                                sg_info.push_str(owner);
                                                sg_info.push(')');
                                            }
                                            found_subgraphs.push(sg_info);
                                        }
                                    }
                                }
                                if !found_subgraphs.is_empty() {
                                    markdown.push_str("\n\n---\n\n**Subgraphs:** ");
                                    markdown.push_str(&found_subgraphs.join(", "));
                                }
                            }
                            SemanticSymbol::Type(ty) => {
                                let mut found_subgraphs = Vec::new();
                                for sg in subgraphs {
                                    if sg.schema.types.contains_key(ty.name()) {
                                        let mut sg_info = sg.name.clone();
                                        if let Some(owner) = &sg.owner {
                                            sg_info.push_str(" (");
                                            sg_info.push_str(owner);
                                            sg_info.push(')');
                                        }
                                        found_subgraphs.push(sg_info);
                                    }
                                }
                                if !found_subgraphs.is_empty() {
                                    markdown.push_str("\n\n---\n\n**Defined in Subgraphs:** ");
                                    markdown.push_str(&found_subgraphs.join(", "));
                                }
                            }
                            _ => {}
                        }
                    }

                    return Some(Hover {
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: markdown,
                        }),
                        range: Some(self.translate_to_file_range(node, offset)),
                    });
                }
            }
        }
        None
    }
}
