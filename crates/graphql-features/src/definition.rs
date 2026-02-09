use ahash::RandomState;
use apollo_compiler::Schema;
use apollo_compiler::schema::ExtendedType;
use dashmap::DashMap;
use graphql_core::document::DocumentState;
use graphql_core::queries::*;
use lsp_types::*;
use std::sync::Arc;
use tree_sitter::{QueryCursor, StreamingIterator};
use url::Url;

pub trait DocumentDefinition {
    fn find_variable_definition(&self, name: &str, position: Position) -> Option<Location>;
    fn get_field_definition_location(
        &self,
        position: Position,
        schema: &Schema,
        documents: &DashMap<Url, Arc<DocumentState>, RandomState>,
        preferred_uris: &[Url],
        fragment_definitions: &DashMap<Arc<str>, ahash::AHashSet<Url>, RandomState>,
    ) -> Option<Location>;
    fn find_definition_in_tree(&self, name: &str) -> Option<Location>;
    fn find_type_definition_in_schema(
        &self,
        name: &str,
        query: &tree_sitter::Query,
    ) -> Option<Location>;
    fn find_enum_value_definition(
        &self,
        position: Position,
        schema: &Schema,
        documents: &DashMap<Url, Arc<DocumentState>, RandomState>,
        preferred_uris: &[Url],
    ) -> Option<Location>;
    fn find_type_condition_definition(
        &self,
        position: Position,
        schema: &Schema,
        documents: &DashMap<Url, Arc<DocumentState>, RandomState>,
        preferred_uris: &[Url],
    ) -> Option<Location>;
    fn find_enum_value_in_schema(
        &self,
        enum_name: &str,
        enum_value_name: &str,
        query: &tree_sitter::Query,
    ) -> Option<Location>;
    fn find_field_in_type_definition(
        &self,
        type_name: &str,
        field_name: &str,
        query: &tree_sitter::Query,
    ) -> Option<Location>;
    fn find_argument_in_field_definition(
        &self,
        type_name: &str,
        field_name: &str,
        arg_name: &str,
        query: &tree_sitter::Query,
    ) -> Option<Location>;
    fn find_input_field_definition(
        &self,
        position: Position,
        schema: &Schema,
        documents: &DashMap<Url, Arc<DocumentState>, RandomState>,
        preferred_uris: &[Url],
    ) -> Option<Location>;
    fn find_input_field_in_type_definition(
        &self,
        type_name: &str,
        field_name: &str,
        query: &tree_sitter::Query,
    ) -> Option<Location>;
}

fn resolve_input_context(
    doc: &DocumentState,
    node: tree_sitter::Node,
    offset: usize,
    schema: &Schema,
) -> Option<(String, Vec<String>)> {
    let mut curr = Some(node);
    let mut field_path = Vec::new();

    while let Some(current_node) = curr {
        if current_node.kind() == "object_field" {
            if let Some(name_node) = doc.find_child_by_kind(current_node, "name") {
                field_path.push(doc.get_node_text(name_node, offset));
            }
        } else if current_node.kind() == "argument" {
            let arg_name = doc
                .find_child_by_kind(current_node, "name")
                .map(|n| doc.get_node_text(n, offset))?;

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
                match &parent_type {
                    ExtendedType::Object(obj) => obj
                        .fields
                        .get(target_name.as_str())?
                        .arguments
                        .iter()
                        .find(|a| a.name.as_str() == arg_name)?
                        .ty
                        .inner_named_type()
                        .to_string(),
                    ExtendedType::Interface(iface) => iface
                        .fields
                        .get(target_name.as_str())?
                        .arguments
                        .iter()
                        .find(|a| a.name.as_str() == arg_name)?
                        .ty
                        .inner_named_type()
                        .to_string(),
                    _ => return None,
                }
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

fn resolve_type_from_path(
    schema: &Schema,
    mut current_type_name: String,
    path: &[String],
) -> Option<String> {
    for segment in path {
        if let Some(ExtendedType::InputObject(io)) = schema.types.get(current_type_name.as_str()) {
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

impl DocumentDefinition for DocumentState {
    fn find_variable_definition(&self, name: &str, position: Position) -> Option<Location> {
        let byte_offset = self.position_to_byte(position);

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();
            let tree_len = root.end_byte();

            if byte_offset >= offset && byte_offset < offset + tree_len {
                let mut node =
                    root.descendant_for_byte_range(byte_offset - offset, byte_offset - offset)?;
                while let Some(parent) = node.parent() {
                    if parent.kind() == "operation_definition"
                        || parent.kind() == "fragment_definition"
                    {
                        let mut walker = parent.walk();
                        for child in parent.children(&mut walker) {
                            if child.kind() == "variable_definitions" {
                                let mut vd_walker = child.walk();
                                for vd in child.children(&mut vd_walker) {
                                    if vd.kind() == "variable_definition"
                                        && let Some(var_node) =
                                            self.find_child_by_kind(vd, "variable")
                                        && self.get_node_text(var_node, offset) == name
                                    {
                                        return Some(Location {
                                            uri: self.uri.clone(),
                                            range: self.translate_to_file_range(var_node, offset),
                                        });
                                    }
                                }
                            }
                        }
                    }
                    node = parent;
                }
            }
        }
        None
    }

    fn get_field_definition_location(
        &self,
        position: Position,
        schema: &Schema,
        documents: &DashMap<Url, Arc<DocumentState>, RandomState>,
        preferred_uris: &[Url],
        _fragment_definitions: &DashMap<Arc<str>, ahash::AHashSet<Url>, RandomState>,
    ) -> Option<Location> {
        let byte_offset = self.position_to_byte(position);

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();
            let tree_len = root.end_byte();

            if byte_offset >= offset && byte_offset < offset + tree_len {
                let local_byte = byte_offset - offset;
                let node = root.descendant_for_byte_range(local_byte, local_byte)?;

                if node.kind() == "name" {
                    let name = self.get_node_text(node, offset);
                    let mut curr = Some(node);
                    while let Some(current_node) = curr {
                        if current_node.kind() == "field" {
                            if let Some(parent_type) =
                                self.find_parent_type_for_node(current_node, offset, schema)
                            {
                                let symbol_query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
                                    let lang = tree_sitter_graphql::LANGUAGE.into();
                                    tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
                                });

                                for p_uri in preferred_uris {
                                    if let Some(doc) =
                                        documents.get(p_uri).map(|r| r.value().clone())
                                        && let Some(loc) = doc.find_field_in_type_definition(
                                            parent_type.name(),
                                            &name,
                                            symbol_query,
                                        )
                                    {
                                        return Some(loc);
                                    }
                                }

                                return self.find_field_in_type_definition(
                                    parent_type.name(),
                                    &name,
                                    symbol_query,
                                );
                            }
                            break;
                        } else if current_node.kind() == "argument" {
                            if let Some(parent) = current_node.parent() {
                                let field_node = if parent.kind() == "arguments" {
                                    parent.parent()
                                } else {
                                    Some(parent)
                                };

                                if let Some(field_node) = field_node
                                    && (field_node.kind() == "field"
                                        || field_node.kind() == "directive")
                                {
                                    if field_node.kind() == "field" {
                                        if let Some(field_name_node) =
                                            self.find_child_by_kind(field_node, "name")
                                        {
                                            let field_name =
                                                self.get_node_text(field_name_node, offset);
                                            if let Some(parent_type) = self
                                                .find_parent_type_for_node(
                                                    field_node, offset, schema,
                                                )
                                            {
                                                let symbol_query = GQL_SYMBOL_QUERY_CACHE
                                                    .get_or_init(|| {
                                                        let lang =
                                                            tree_sitter_graphql::LANGUAGE.into();
                                                        tree_sitter::Query::new(
                                                            &lang,
                                                            GQL_SYMBOL_QUERY,
                                                        )
                                                        .unwrap()
                                                    });

                                                for p_uri in preferred_uris {
                                                    if let Some(doc) = documents
                                                        .get(p_uri)
                                                        .map(|r| r.value().clone())
                                                        && let Some(loc) = doc
                                                            .find_argument_in_field_definition(
                                                                parent_type.name(),
                                                                &field_name,
                                                                &name,
                                                                symbol_query,
                                                            )
                                                    {
                                                        return Some(loc);
                                                    }
                                                }

                                                return self.find_argument_in_field_definition(
                                                    parent_type.name(),
                                                    &field_name,
                                                    &name,
                                                    symbol_query,
                                                );
                                            }
                                        }
                                    } else {
                                        // Directive argument
                                        let directive_name_node =
                                            self.find_child_by_kind(field_node, "name")?;
                                        let directive_name =
                                            self.get_node_text(directive_name_node, offset);

                                        let symbol_query =
                                            GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
                                                let lang = tree_sitter_graphql::LANGUAGE.into();
                                                tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY)
                                                    .unwrap()
                                            });

                                        for p_uri in preferred_uris {
                                            if let Some(doc) =
                                                documents.get(p_uri).map(|r| r.value().clone())
                                                && let Some(loc) = doc
                                                    .find_argument_in_field_definition(
                                                        &directive_name,
                                                        "", // Empty field name for directive
                                                        &name,
                                                        symbol_query,
                                                    )
                                            {
                                                return Some(loc);
                                            }
                                        }

                                        return self.find_argument_in_field_definition(
                                            &directive_name,
                                            "",
                                            &name,
                                            symbol_query,
                                        );
                                    }
                                }
                            }
                            break;
                        }
                        curr = current_node.parent();
                    }
                }
            }
        }
        None
    }

    fn find_definition_in_tree(&self, name: &str) -> Option<Location> {
        let query = GQL_DEFINITION_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_DEFINITION_QUERY).unwrap()
        });

        let mut cursor = QueryCursor::new();
        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let mut matches =
                cursor.matches(query, block.tree.root_node(), |n: tree_sitter::Node| {
                    let start = n.start_byte();
                    let end = n.end_byte();
                    self.rope
                        .byte_slice((start + offset)..(end + offset))
                        .chunks()
                });

            while let Some(m) = matches.next() {
                for cap in m.captures {
                    if self.get_node_text(cap.node, offset) == name {
                        return Some(Location {
                            uri: self.uri.clone(),
                            range: self.translate_to_file_range(cap.node, offset),
                        });
                    }
                }
            }
        }
        None
    }

    fn find_type_definition_in_schema(
        &self,
        name: &str,
        query: &tree_sitter::Query,
    ) -> Option<Location> {
        let mut cursor = QueryCursor::new();
        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let mut matches =
                cursor.matches(query, block.tree.root_node(), |n: tree_sitter::Node| {
                    let start = n.start_byte();
                    let end = n.end_byte();
                    self.rope
                        .byte_slice((start + offset)..(end + offset))
                        .chunks()
                });

            while let Some(m) = matches.next() {
                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    if cap_name == "symbol.name" && self.get_node_text(cap.node, offset) == name {
                        return Some(Location {
                            uri: self.uri.clone(),
                            range: self.translate_to_file_range(cap.node, offset),
                        });
                    }
                }
            }
        }
        None
    }

    fn find_enum_value_in_schema(
        &self,
        enum_name: &str,
        enum_value_name: &str,
        query: &tree_sitter::Query,
    ) -> Option<Location> {
        let mut cursor = QueryCursor::new();

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let mut matches =
                cursor.matches(query, block.tree.root_node(), |node: tree_sitter::Node| {
                    let start = node.start_byte();
                    let end = node.end_byte();
                    self.rope
                        .byte_slice((start + offset)..(end + offset))
                        .chunks()
                });

            while let Some(m) = matches.next() {
                let mut name = None;
                let mut container_node = None;

                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    if cap_name == "symbol.name" {
                        name = Some(self.get_node_text(cap.node, offset));
                    } else if cap_name == "symbol.container" {
                        container_node = Some(cap.node);
                    }
                }

                if let Some(n) = name
                    && n == enum_name
                    && let Some(container) = container_node
                {
                    let ref_query = GQL_REFERENCES_QUERY_CACHE.get_or_init(|| {
                        let lang = tree_sitter_graphql::LANGUAGE.into();
                        tree_sitter::Query::new(&lang, GQL_REFERENCES_QUERY).unwrap()
                    });
                    let mut ref_cursor = QueryCursor::new();
                    let mut ref_matches =
                        ref_cursor.matches(ref_query, container, |node: tree_sitter::Node| {
                            let start = node.start_byte();
                            let end = node.end_byte();
                            self.rope
                                .byte_slice((start + offset)..(end + offset))
                                .chunks()
                        });

                    while let Some(rm) = ref_matches.next() {
                        let mut is_definition = false;
                        let mut name_node = None;

                        for rcap in rm.captures {
                            let rcap_name = ref_query.capture_names()[rcap.index as usize];
                            if rcap_name == "definition" {
                                is_definition = true;
                            } else if rcap_name == "name" {
                                name_node = Some(rcap.node);
                            }
                        }

                        if is_definition
                            && let Some(nn) = name_node
                            && self.get_node_text(nn, offset) == enum_value_name
                        {
                            return Some(Location {
                                uri: self.uri.clone(),
                                range: self.translate_to_file_range(nn, offset),
                            });
                        }
                    }
                }
            }
        }
        None
    }

    fn find_enum_value_definition(
        &self,
        position: Position,
        schema: &Schema,
        documents: &DashMap<Url, Arc<DocumentState>, RandomState>,
        preferred_uris: &[Url],
    ) -> Option<Location> {
        let byte_offset = self.position_to_byte(position);

        let block = self.get_graphql_trees().iter().find(|b| {
            let root = b.tree.root_node();
            let tree_len = root.end_byte();
            byte_offset >= b.offset && byte_offset < b.offset + tree_len
        })?;

        let offset = block.offset;
        let root = block.tree.root_node();
        let local_byte = byte_offset - offset;
        let mut node = root.descendant_for_byte_range(local_byte, local_byte)?;

        if node.kind() == "name"
            && let Some(parent) = node.parent()
            && parent.kind() == "enum_value"
        {
            node = parent;
        }

        if node.kind() != "enum_value" {
            return None;
        }

        let enum_value_name = self.get_node_text(node, offset);

        let (root_type, path) = resolve_input_context(self, node, offset, schema)?;
        let enum_type_name = resolve_type_from_path(schema, root_type, &path)?;

        if let Some(ExtendedType::Enum(enum_type)) = schema.types.get(enum_type_name.as_str()) {
            let enum_name = enum_type.name.as_str();

            let symbol_query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
                let lang = tree_sitter_graphql::LANGUAGE.into();
                tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
            });

            for p_uri in preferred_uris {
                if let Some(doc) = documents.get(p_uri).map(|r| r.value().clone())
                    && let Some(loc) =
                        doc.find_enum_value_in_schema(enum_name, &enum_value_name, symbol_query)
                {
                    return Some(loc);
                }
            }

            return self.find_enum_value_in_schema(enum_name, &enum_value_name, symbol_query);
        }

        None
    }

    fn find_type_condition_definition(
        &self,
        position: Position,
        _schema: &Schema,
        documents: &DashMap<Url, Arc<DocumentState>, RandomState>,
        preferred_uris: &[Url],
    ) -> Option<Location> {
        let byte_offset = self.position_to_byte(position);

        let block = self.get_graphql_trees().iter().find(|b| {
            let root = b.tree.root_node();
            let tree_len = root.end_byte();
            byte_offset >= b.offset && byte_offset < b.offset + tree_len
        })?;

        let offset = block.offset;
        let root = block.tree.root_node();
        let local_byte = byte_offset - offset;
        let node = root.descendant_for_byte_range(local_byte, local_byte)?;

        let mut curr = Some(node);
        while let Some(current_node) = curr {
            if (current_node.kind() == "inline_fragment"
                || current_node.kind() == "fragment_definition")
                && let Some(type_name) = self.get_fragment_type_condition(current_node, offset)
            {
                // Ensure the cursor is actually on the type name or within the type condition
                let mut is_on_type_condition = false;
                if let Some(tc_node) = self.find_child_by_kind(current_node, "type_condition") {
                    let start = tc_node.start_byte();
                    let end = tc_node.end_byte();
                    if local_byte >= start && local_byte < end {
                        is_on_type_condition = true;
                    }
                }

                if !is_on_type_condition {
                    return None;
                }

                let symbol_query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
                    let lang = tree_sitter_graphql::LANGUAGE.into();
                    tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
                });

                for p_uri in preferred_uris {
                    if let Some(doc) = documents.get(p_uri).map(|r| r.value().clone())
                        && let Some(loc) =
                            doc.find_type_definition_in_schema(&type_name, symbol_query)
                    {
                        return Some(loc);
                    }
                }

                return self.find_type_definition_in_schema(&type_name, symbol_query);
            }
            curr = current_node.parent();
        }

        None
    }

    fn find_field_in_type_definition(
        &self,
        type_name: &str,
        field_name: &str,
        query: &tree_sitter::Query,
    ) -> Option<Location> {
        let mut cursor = QueryCursor::new();
        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let mut matches =
                cursor.matches(query, block.tree.root_node(), |n: tree_sitter::Node| {
                    let start = n.start_byte();
                    let end = n.end_byte();
                    self.rope
                        .byte_slice((start + offset)..(end + offset))
                        .chunks()
                });

            while let Some(m) = matches.next() {
                let mut name = None;
                let mut container_node = None;

                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    if cap_name == "symbol.name" {
                        name = Some(self.get_node_text(cap.node, offset));
                    } else if cap_name == "symbol.container" {
                        container_node = Some(cap.node);
                    }
                }

                if let Some(n) = name
                    && n == type_name
                    && let Some(container) = container_node
                {
                    let mut walker = container.walk();
                    for child in container.children(&mut walker) {
                        if child.kind().contains("field") {
                            let mut fd_walker = child.walk();
                            for field_def in child.children(&mut fd_walker) {
                                if field_def.kind() == "field_definition"
                                    && let Some(f_name_node) =
                                        self.find_child_by_kind(field_def, "name")
                                    && self.get_node_text(f_name_node, offset) == field_name
                                {
                                    return Some(Location {
                                        uri: self.uri.clone(),
                                        range: self.translate_to_file_range(f_name_node, offset),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn find_argument_in_field_definition(
        &self,
        type_name: &str,
        field_name: &str,
        arg_name: &str,
        query: &tree_sitter::Query,
    ) -> Option<Location> {
        let mut cursor = QueryCursor::new();
        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let mut matches =
                cursor.matches(query, block.tree.root_node(), |n: tree_sitter::Node| {
                    let start = n.start_byte();
                    let end = n.end_byte();
                    self.rope
                        .byte_slice((start + offset)..(end + offset))
                        .chunks()
                });

            while let Some(m) = matches.next() {
                let mut name = None;
                let mut container_node = None;

                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    if cap_name == "symbol.name" {
                        name = Some(self.get_node_text(cap.node, offset));
                    } else if cap_name == "symbol.container" {
                        container_node = Some(cap.node);
                    }
                }

                if let Some(n) = name
                    && n == type_name
                    && let Some(container) = container_node
                {
                    let mut walker = container.walk();
                    for child in container.children(&mut walker) {
                        if child.kind().contains("arguments_definition") {
                            let mut ad_walker = child.walk();
                            for arg_def in child.children(&mut ad_walker) {
                                if arg_def.kind() == "input_value_definition"
                                    && let Some(a_name_node) =
                                        self.find_child_by_kind(arg_def, "name")
                                    && self.get_node_text(a_name_node, offset) == arg_name
                                {
                                    return Some(Location {
                                        uri: self.uri.clone(),
                                        range: self.translate_to_file_range(a_name_node, offset),
                                    });
                                }
                            }
                        } else if child.kind().contains("field") {
                            let mut fd_walker = child.walk();
                            for field_def in child.children(&mut fd_walker) {
                                if field_def.kind() == "field_definition"
                                    && let Some(f_name_node) =
                                        self.find_child_by_kind(field_def, "name")
                                    && self.get_node_text(f_name_node, offset) == field_name
                                {
                                    // Found the field, now find the argument
                                    if let Some(ad_node) =
                                        self.find_child_by_kind(field_def, "arguments_definition")
                                    {
                                        let mut ad_walker = ad_node.walk();
                                        for arg_def in ad_node.children(&mut ad_walker) {
                                            if arg_def.kind() == "input_value_definition"
                                                && let Some(a_name_node) =
                                                    self.find_child_by_kind(arg_def, "name")
                                                && self.get_node_text(a_name_node, offset)
                                                    == arg_name
                                            {
                                                return Some(Location {
                                                    uri: self.uri.clone(),
                                                    range: self.translate_to_file_range(
                                                        a_name_node,
                                                        offset,
                                                    ),
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn find_input_field_definition(
        &self,
        position: Position,
        schema: &Schema,
        documents: &DashMap<Url, Arc<DocumentState>, RandomState>,
        preferred_uris: &[Url],
    ) -> Option<Location> {
        let byte_offset = self.position_to_byte(position);

        let block = self.get_graphql_trees().iter().find(|b| {
            let root = b.tree.root_node();
            let tree_len = root.end_byte();
            byte_offset >= b.offset && byte_offset < b.offset + tree_len
        })?;

        let offset = block.offset;
        let root = block.tree.root_node();
        let local_byte = byte_offset - offset;
        let node = root.descendant_for_byte_range(local_byte, local_byte)?;

        if node.kind() != "name" {
            return None;
        }

        let field_name = self.get_node_text(node, offset);

        let (root_type, path) = resolve_input_context(self, node, offset, schema)?;

        // We want the type containing the current field.
        // The path contains the current field as the last element.
        if path.is_empty() {
            return None;
        }
        let container_type_name =
            resolve_type_from_path(schema, root_type, &path[..path.len() - 1])?;

        let symbol_query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
        });

        for p_uri in preferred_uris {
            if let Some(doc) = documents.get(p_uri).map(|r| r.value().clone())
                && let Some(loc) = doc.find_input_field_in_type_definition(
                    &container_type_name,
                    &field_name,
                    symbol_query,
                )
            {
                return Some(loc);
            }
        }

        self.find_input_field_in_type_definition(&container_type_name, &field_name, symbol_query)
    }

    fn find_input_field_in_type_definition(
        &self,
        type_name: &str,
        field_name: &str,
        query: &tree_sitter::Query,
    ) -> Option<Location> {
        let mut cursor = QueryCursor::new();
        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let mut matches =
                cursor.matches(query, block.tree.root_node(), |n: tree_sitter::Node| {
                    let start = n.start_byte();
                    let end = n.end_byte();
                    self.rope
                        .byte_slice((start + offset)..(end + offset))
                        .chunks()
                });

            while let Some(m) = matches.next() {
                let mut name = None;
                let mut container_node = None;

                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    if cap_name == "symbol.name" {
                        name = Some(self.get_node_text(cap.node, offset));
                    } else if cap_name == "symbol.container" {
                        container_node = Some(cap.node);
                    }
                }

                if let Some(n) = name
                    && n == type_name
                    && let Some(container) = container_node
                {
                    let mut walker = container.walk();
                    for child in container.children(&mut walker) {
                        if child.kind().contains("input_field") {
                            let mut if_walker = child.walk();
                            for field_def in child.children(&mut if_walker) {
                                if field_def.kind() == "input_value_definition"
                                    && let Some(f_name_node) =
                                        self.find_child_by_kind(field_def, "name")
                                    && self.get_node_text(f_name_node, offset) == field_name
                                {
                                    return Some(Location {
                                        uri: self.uri.clone(),
                                        range: self.translate_to_file_range(f_name_node, offset),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }
}
