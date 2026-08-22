use crate::shared::markdown_utils::*;
use crate::shared::schema_utils;
use crate::shared::type_resolver::{self, SemanticSymbol};
use ahash::AHashSet;
use apollo_compiler::Schema;
use graphox_core::document::{DocumentState, FragmentDef, GraphQLBlock};
use graphox_core::schema::SloClass;
use ls_types::*;
use std::sync::Arc;

pub trait DocumentHover {
    fn get_hover_info(
        &self,
        position: Position,
        schema: &Schema,
        subgraphs: Option<&[graphox_core::schema::SubgraphInfo]>,
        _documents: &graphox_core::types::DocumentsMap,
    ) -> Option<Hover>;

    #[allow(clippy::too_many_arguments)]
    fn calculate_worst_slo_for_selection_set(
        &self,
        node: tree_sitter::Node,
        offset: usize,
        current_type: &apollo_compiler::schema::ExtendedType,
        schema: &Schema,
        subgraphs: &[graphox_core::schema::SubgraphInfo],
        fragment_index: &ahash::AHashMap<Arc<str>, Vec<(Arc<DocumentState>, FragmentDef)>>,
        _documents: &graphox_core::types::DocumentsMap,
        visited_fragments: &mut AHashSet<Arc<str>>,
    ) -> Option<SloClass>;

    fn find_containing_operation_node(
        &self,
        position: Position,
    ) -> Option<(tree_sitter::Node<'_>, usize)>;
    fn get_fragment_spreads_in_node(&self, node: tree_sitter::Node, offset: usize)
    -> Vec<Arc<str>>;
}

impl DocumentHover for DocumentState {
    fn calculate_worst_slo_for_selection_set(
        &self,
        node: tree_sitter::Node,
        offset: usize,
        current_type: &apollo_compiler::schema::ExtendedType,
        schema: &Schema,
        subgraphs: &[graphox_core::schema::SubgraphInfo],
        fragment_index: &ahash::AHashMap<Arc<str>, Vec<(Arc<DocumentState>, FragmentDef)>>,
        _documents: &graphox_core::types::DocumentsMap,
        visited_fragments: &mut AHashSet<Arc<str>>,
    ) -> Option<SloClass> {
        let mut worst: Option<SloClass> = None;

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "selection" {
                let selection_node = if child.kind() == "selection" {
                    self.find_child_by_kind(child, "field")
                        .or_else(|| self.find_child_by_kind(child, "fragment_spread"))
                        .or_else(|| self.find_child_by_kind(child, "inline_fragment"))
                } else {
                    Some(child)
                };

                if let Some(selection) = selection_node {
                    match selection.kind() {
                        "field" => {
                            let field_name =
                                if let Some(n) = self.find_child_by_kind(selection, "name") {
                                    self.get_node_text(n, offset)
                                } else {
                                    continue;
                                };

                            if field_name == "__typename" {
                                continue;
                            }

                            let field_def = match current_type {
                                apollo_compiler::schema::ExtendedType::Object(obj) => {
                                    obj.fields.get(field_name.as_str())
                                }
                                apollo_compiler::schema::ExtendedType::Interface(iface) => {
                                    iface.fields.get(field_name.as_str())
                                }
                                _ => None,
                            };

                            if let Some(field_def) = field_def {
                                // Check field SLO in subgraphs
                                for sg in subgraphs {
                                    if let Some(sg_ty) =
                                        sg.schema.types.get(current_type.name().as_str())
                                    {
                                        let has_field = match sg_ty {
                                            apollo_compiler::schema::ExtendedType::Object(obj) => {
                                                obj.fields.contains_key(field_name.as_str())
                                            }
                                            apollo_compiler::schema::ExtendedType::Interface(
                                                iface,
                                            ) => iface.fields.contains_key(field_name.as_str()),
                                            _ => false,
                                        };

                                        if has_field {
                                            let slo = sg
                                                .field_slos
                                                .get(current_type.name().as_str())
                                                .and_then(|type_slos| {
                                                    type_slos.get(field_name.as_str()).copied()
                                                })
                                                .or(sg.schema_slo);

                                            if let Some(slo) = slo {
                                                worst = Some(worst.map_or(slo, |w| w.worst(slo)));
                                            }
                                        }
                                    }
                                }

                                // Recurse into sub-selection if present
                                if let Some(sub_selection) =
                                    self.find_child_by_kind(selection, "selection_set")
                                    && let Some(field_type) =
                                        schema.types.get(field_def.ty.inner_named_type().as_str())
                                    && let Some(slo) = self.calculate_worst_slo_for_selection_set(
                                        sub_selection,
                                        offset,
                                        field_type,
                                        schema,
                                        subgraphs,
                                        fragment_index,
                                        _documents,
                                        visited_fragments,
                                    )
                                {
                                    worst = Some(worst.map_or(slo, |w| w.worst(slo)));
                                }
                            }
                        }
                        "fragment_spread" => {
                            let frag_name: Option<Arc<str>> = self
                                .find_child_by_kind(selection, "fragment_name")
                                .and_then(|n| self.find_child_by_kind(n, "name"))
                                .map(|n| self.get_node_text(n, offset).into());

                            let frag_name = if let Some(n) = frag_name {
                                n
                            } else {
                                continue;
                            };

                            if !visited_fragments.insert(frag_name.clone()) {
                                continue;
                            }

                            // Find fragment definition using indexed map
                            if let Some(candidates) = fragment_index.get(&frag_name) {
                                for (doc, frag) in candidates {
                                    for block in doc.get_graphql_trees() {
                                        let block: &GraphQLBlock = block;
                                        let root = block.tree.root_node();
                                        let mut cursor = root.walk();
                                        for node in root.children(&mut cursor) {
                                            if node.kind() == "fragment_definition" {
                                                let name = doc
                                                    .find_child_by_kind(node, "fragment_name")
                                                    .and_then(|n| doc.find_child_by_kind(n, "name"))
                                                    .map(|n| doc.get_node_text(n, block.offset));

                                                if name.as_deref() == Some(frag.name.as_ref())
                                                    && let Some(selection) = doc
                                                        .find_child_by_kind(node, "selection_set")
                                                    && let Some(type_cond) = schema
                                                        .types
                                                        .get(frag.type_condition.as_ref())
                                                    && let Some(slo) = doc
                                                        .calculate_worst_slo_for_selection_set(
                                                            selection,
                                                            block.offset,
                                                            type_cond,
                                                            schema,
                                                            subgraphs,
                                                            fragment_index,
                                                            _documents,
                                                            visited_fragments,
                                                        )
                                                {
                                                    worst =
                                                        Some(worst.map_or(slo, |w| w.worst(slo)));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        "inline_fragment" => {
                            let type_cond_node = self
                                .find_child_by_kind(selection, "type_condition")
                                .and_then(|tc| self.find_child_by_kind(tc, "named_type"))
                                .and_then(|nt| self.find_child_by_kind(nt, "name"));

                            let target_type = if let Some(tc) = type_cond_node {
                                schema.types.get(self.get_node_text(tc, offset).as_str())
                            } else {
                                Some(current_type)
                            };

                            if let Some(target_type) = target_type
                                && let Some(selection) =
                                    self.find_child_by_kind(selection, "selection_set")
                                && let Some(slo) = self.calculate_worst_slo_for_selection_set(
                                    selection,
                                    offset,
                                    target_type,
                                    schema,
                                    subgraphs,
                                    fragment_index,
                                    _documents,
                                    visited_fragments,
                                )
                            {
                                worst = Some(worst.map_or(slo, |w| w.worst(slo)));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        worst
    }

    fn get_hover_info(
        &self,
        position: Position,
        schema: &Schema,
        subgraphs: Option<&[graphox_core::schema::SubgraphInfo]>,
        documents: &graphox_core::types::DocumentsMap,
    ) -> Option<Hover> {
        let byte_offset = self.position_to_byte(position);

        // Build fragment index once per hover request
        let mut fragment_index = ahash::AHashMap::default();
        for entry in documents.iter() {
            let doc = entry.value();
            for frag in doc.fragments() {
                fragment_index
                    .entry(frag.name.clone())
                    .or_insert_with(Vec::new)
                    .push((doc.clone(), frag.clone()));
            }
        }

        for block in self.get_graphql_trees() {
            let block: &GraphQLBlock = block;
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
                    let markdown = match &symbol {
                        SemanticSymbol::Field {
                            parent_type,
                            field_def,
                            alias,
                        } => {
                            let mut sg_markdown = String::new();
                            if let Some(project_subgraphs) = subgraphs {
                                let mut found_subgraphs = Vec::new();
                                for sg in project_subgraphs {
                                    if let Some(sg_ty) =
                                        sg.schema.types.get(parent_type.name().as_str())
                                    {
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

                                            // Add SLO info if available
                                            let slo = sg
                                                .field_slos
                                                .get(parent_type.name().as_str())
                                                .and_then(|type_slos| {
                                                    type_slos.get(field_def.name.as_str()).copied()
                                                })
                                                .or(sg.schema_slo);

                                            if let Some(slo) = slo {
                                                sg_info.push_str(" [SLO: ");
                                                sg_info.push_str(slo.as_str());
                                                sg_info.push(']');
                                            }

                                            found_subgraphs.push(sg_info);
                                        }
                                    }
                                }
                                if !found_subgraphs.is_empty() {
                                    sg_markdown = format!(
                                        "\n\n---\n\n**Subgraphs:** {}",
                                        found_subgraphs.join(", ")
                                    );
                                }
                            }

                            let deprecation_reason =
                                schema_utils::get_deprecation_reason(&field_def.directives, schema);

                            let base_markdown = if let Some(alias_name) = alias {
                                describe_field_markdown_with_alias(
                                    parent_type.name(),
                                    field_def.name.as_str(),
                                    alias_name,
                                    field_def.ty.to_string().as_str(),
                                    field_def.description.as_deref(),
                                    &field_def.arguments,
                                    deprecation_reason,
                                )
                            } else {
                                describe_field_markdown(
                                    parent_type.name(),
                                    field_def.name.as_str(),
                                    field_def.ty.to_string().as_str(),
                                    field_def.description.as_deref(),
                                    &field_def.arguments,
                                    deprecation_reason,
                                )
                            };

                            base_markdown + &sg_markdown
                        }
                        SemanticSymbol::BuiltinField { name, parent_type } => {
                            if let Some(parent_ty) = schema.types.get(parent_type.name()) {
                                describe_builtin_field_markdown(name, parent_ty, schema)
                            } else {
                                format!("### field {}\n---\nBuilt-in field", name)
                            }
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
                        SemanticSymbol::Type(ty) => {
                            let mut extra = String::new();
                            let mut implementations = Vec::new();

                            if let Some(project_subgraphs) = subgraphs {
                                let mut found_subgraphs = Vec::new();
                                for sg in project_subgraphs {
                                    if sg.schema.types.contains_key(ty.name().as_str()) {
                                        let mut sg_info = sg.name.clone();
                                        if let Some(owner) = &sg.owner {
                                            sg_info.push_str(" (");
                                            sg_info.push_str(owner);
                                            sg_info.push(')');
                                        }

                                        if let Some(slo) = sg.schema_slo {
                                            sg_info.push_str(" [SLO: ");
                                            sg_info.push_str(slo.as_str());
                                            sg_info.push(']');
                                        }

                                        found_subgraphs.push(sg_info);
                                    }
                                }
                                if !found_subgraphs.is_empty() {
                                    extra = format!(
                                        "\n\n---\n\n**Defined in Subgraphs:** {}",
                                        found_subgraphs.join(", ")
                                    );
                                }
                            }

                            if let apollo_compiler::schema::ExtendedType::Interface(_) = ty {
                                for (t_name, t_def) in &schema.types {
                                    match t_def {
                                        apollo_compiler::schema::ExtendedType::Object(obj)
                                            if obj
                                                .implements_interfaces
                                                .iter()
                                                .any(|i| i.as_str() == ty.name().as_str()) =>
                                        {
                                            implementations.push(t_name.to_string());
                                        }
                                        apollo_compiler::schema::ExtendedType::Interface(iface)
                                            if iface
                                                .implements_interfaces
                                                .iter()
                                                .any(|i| i.as_str() == ty.name().as_str()) =>
                                        {
                                            implementations.push(t_name.to_string());
                                        }
                                        _ => {}
                                    }
                                }
                            }

                            if !implementations.is_empty() {
                                implementations.sort();
                            }

                            describe_full_type_markdown(
                                ty.name(),
                                ty,
                                if implementations.is_empty() {
                                    None
                                } else {
                                    Some(&implementations)
                                },
                            ) + &extra
                        }
                        SemanticSymbol::Variable { name, ty_text } => {
                            describe_variable_markdown(name, ty_text)
                        }
                        SemanticSymbol::EnumValue { enum_name, val_def } => {
                            let deprecation_reason =
                                schema_utils::get_deprecation_reason(&val_def.directives, schema);
                            describe_enum_value_markdown(
                                enum_name,
                                val_def.value.as_str(),
                                val_def.description.as_deref(),
                                deprecation_reason,
                            )
                        }
                        SemanticSymbol::Literal {
                            kind,
                            expected_type,
                        } => describe_literal_markdown(kind, expected_type),
                        SemanticSymbol::DefaultValue { ty_text } => {
                            describe_default_value_markdown(ty_text)
                        }
                        SemanticSymbol::Operation {
                            op_type,
                            name,
                            variables,
                            description,
                        } => {
                            // Find the operation definition node again to get its selection set
                            let mut current = node;
                            while current.kind() != "operation_definition" {
                                if let Some(parent) = current.parent() {
                                    current = parent;
                                } else {
                                    break;
                                }
                            }

                            let mut md = describe_operation_markdown(
                                op_type,
                                name.as_deref(),
                                variables,
                                description.as_deref(),
                            );

                            if current.kind() == "operation_definition"
                                && let Some(selection) =
                                    self.find_child_by_kind(current, "selection_set")
                            {
                                let root_type_name = match op_type.as_ref() {
                                    "query" => schema
                                        .root_operation(apollo_compiler::ast::OperationType::Query)
                                        .map(|t| t.as_str()),
                                    "mutation" => schema
                                        .root_operation(
                                            apollo_compiler::ast::OperationType::Mutation,
                                        )
                                        .map(|t| t.as_str()),
                                    "subscription" => schema
                                        .root_operation(
                                            apollo_compiler::ast::OperationType::Subscription,
                                        )
                                        .map(|t| t.as_str()),
                                    _ => None,
                                };

                                if let Some(root_name) = root_type_name
                                    && let Some(root_type) = schema.types.get(root_name)
                                    && let Some(project_subgraphs) = subgraphs
                                {
                                    let mut visited = AHashSet::default();
                                    if let Some(slo) = self.calculate_worst_slo_for_selection_set(
                                        selection,
                                        offset,
                                        root_type,
                                        schema,
                                        project_subgraphs,
                                        &fragment_index,
                                        documents,
                                        &mut visited,
                                    ) {
                                        md.push_str("\n\n---\n\n**Worst SLO:** ");
                                        md.push_str(slo.as_str());
                                    }
                                }
                            }
                            md
                        }
                        SemanticSymbol::Fragment {
                            name,
                            type_condition,
                            description,
                        } => {
                            // Find the fragment definition node again to get its selection set
                            let mut current = node;
                            while current.kind() != "fragment_definition" {
                                if let Some(parent) = current.parent() {
                                    current = parent;
                                } else {
                                    break;
                                }
                            }

                            if current.kind() != "fragment_definition" {
                                return None;
                            }

                            let mut md = describe_fragment_markdown(
                                name,
                                type_condition,
                                description.as_deref(),
                            );

                            if current.kind() == "fragment_definition"
                                && let Some(selection) =
                                    self.find_child_by_kind(current, "selection_set")
                                && let Some(type_cond) = schema.types.get(type_condition.as_str())
                                && let Some(project_subgraphs) = subgraphs
                            {
                                let mut visited = AHashSet::default();
                                // Ensure we don't recurse into ourselves
                                visited.insert(name.clone().into());

                                if let Some(slo) = self.calculate_worst_slo_for_selection_set(
                                    selection,
                                    offset,
                                    type_cond,
                                    schema,
                                    project_subgraphs,
                                    &fragment_index,
                                    documents,
                                    &mut visited,
                                ) {
                                    md.push_str("\n\n---\n\n**Worst SLO:** ");
                                    md.push_str(slo.as_str());
                                }
                            }
                            md
                        }
                        SemanticSymbol::LocalSymbol { name, description } => {
                            format!("### {}\n---\n{}", name, description)
                        }
                        SemanticSymbol::InputObjectField {
                            parent_type,
                            field_def,
                        } => describe_field_markdown(
                            parent_type.name().as_str(),
                            field_def.name.as_str(),
                            field_def.ty.to_string().as_str(),
                            field_def.description.as_deref(),
                            &[],
                            None,
                        ),
                        SemanticSymbol::TypeExtension {
                            type_name,
                            adds_fields,
                            ..
                        } => {
                            let mut md = format!("### extends {}\n---\nType extension", type_name);
                            if !adds_fields.is_empty() {
                                md.push_str("\n\nAdds: ");
                                let fields: Vec<String> =
                                    adds_fields.iter().map(|f| format!("`{}`", f)).collect();
                                md.push_str(&fields.join(", "));
                            }
                            md
                        }
                    };

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

    fn find_containing_operation_node(
        &self,
        position: Position,
    ) -> Option<(tree_sitter::Node<'_>, usize)> {
        let offset = self.position_to_byte(position);
        for block in self.get_graphql_trees() {
            let block: &GraphQLBlock = block;
            if offset >= block.offset && offset <= block.offset + block.tree.root_node().end_byte()
            {
                let mut curr = block
                    .tree
                    .root_node()
                    .descendant_for_byte_range(offset - block.offset, offset - block.offset);
                while let Some(node) = curr {
                    if node.kind() == "operation_definition" {
                        return Some((node, block.offset));
                    }
                    curr = node.parent();
                }
            }
        }
        None
    }

    fn get_fragment_spreads_in_node(
        &self,
        node: tree_sitter::Node,
        offset: usize,
    ) -> Vec<Arc<str>> {
        let mut spreads = Vec::new();
        let mut cursor = node.walk();
        let mut stack = vec![node];

        while let Some(current) = stack.pop() {
            if current.kind() == "fragment_spread"
                && let Some(name_node) = self
                    .find_child_by_kind(current, "fragment_name")
                    .and_then(|n| self.find_child_by_kind(n, "name"))
            {
                spreads.push(self.get_node_text(name_node, offset).into());
            }
            cursor.reset(current);
            for child in current.children(&mut cursor) {
                stack.push(child);
            }
        }
        spreads
    }
}
