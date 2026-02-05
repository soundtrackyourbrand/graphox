use crate::document::DocumentState;
use crate::queries::*;
use tower_lsp::lsp_types::*;
use tree_sitter::{QueryCursor, StreamingIterator};

impl DocumentState {
    pub fn get_symbol_at_position(&self, position: Position) -> Option<String> {
        let byte_offset = self.position_to_byte(position);

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();
            let tree_len = root.end_byte();

            if byte_offset >= offset && byte_offset < offset + tree_len {
                let local_byte = byte_offset - offset;
                let trigger_node = root.descendant_for_byte_range(local_byte, local_byte)?;

                let mut target_node = trigger_node;
                if target_node.kind() == "name"
                    && let Some(parent) = target_node.parent()
                    && parent.kind() == "variable"
                {
                    target_node = parent;
                }

                if target_node.kind() == "name" || target_node.kind() == "variable" {
                    return Some(
                        self.rope
                            .slice(
                                self.rope.byte_to_char(target_node.start_byte() + offset)
                                    ..self.rope.byte_to_char(target_node.end_byte() + offset),
                            )
                            .to_string(),
                    );
                }
            }
        }
        None
    }

    pub fn find_containing_operation_node(
        &self,
        position: Position,
    ) -> Option<(tree_sitter::Node<'_>, usize)> {
        let byte_offset = self.position_to_byte(position);

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();
            let tree_len = root.end_byte();

            if byte_offset >= offset && byte_offset < offset + tree_len {
                let local_byte = byte_offset - offset;
                let trigger_node = root.descendant_for_byte_range(local_byte, local_byte)?;

                // Find containing operation
                let mut curr = trigger_node;
                while curr.kind() != "operation_definition" {
                    if let Some(parent) = curr.parent() {
                        curr = parent;
                    } else {
                        break;
                    }
                }

                if curr.kind() == "operation_definition" {
                    return Some((curr, offset));
                }
            }
        }
        None
    }

    pub fn find_variable_definition(
        &self,
        symbol_name: &str,
        position: Position,
    ) -> Option<Location> {
        if !symbol_name.starts_with('$') {
            return None;
        }

        if let Some((op_node, offset)) = self.find_containing_operation_node(position) {
            // Search within this operation ONLY
            let query = GQL_DEFINITION_QUERY_CACHE.get_or_init(|| {
                let lang = tree_sitter_graphql::LANGUAGE.into();
                tree_sitter::Query::new(&lang, GQL_DEFINITION_QUERY).unwrap()
            });

            let mut cursor = QueryCursor::new();
            let mut matches = cursor.matches(query, op_node, |node: tree_sitter::Node| {
                let start = node.start_byte();
                let end = node.end_byte();
                self.rope
                    .byte_slice((start + offset)..(end + offset))
                    .chunks()
            });

            while let Some(m) = matches.next() {
                let name_node = m.captures[0].node;
                let name = self
                    .rope
                    .slice(
                        self.rope.byte_to_char(name_node.start_byte() + offset)
                            ..self.rope.byte_to_char(name_node.end_byte() + offset),
                    )
                    .to_string();

                if name == symbol_name {
                    let range = self.translate_to_file_range(name_node, offset);
                    return Some(Location {
                        uri: self.uri.clone(),
                        range,
                    });
                }
            }
        }

        None
    }

    pub fn find_definition_in_tree(&self, target_name: &str) -> Option<Location> {
        if target_name.starts_with('$') {
            return None;
        }

        let query = GQL_DEFINITION_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_DEFINITION_QUERY).unwrap()
        });

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
                let name_node = m.captures[0].node;
                let name = self
                    .rope
                    .slice(
                        self.rope.byte_to_char(name_node.start_byte() + offset)
                            ..self.rope.byte_to_char(name_node.end_byte() + offset),
                    )
                    .to_string();

                if name == target_name {
                    let range = self.translate_to_file_range(name_node, offset);
                    return Some(Location {
                        uri: self.uri.clone(),
                        range,
                    });
                }
            }
        }

        None
    }

    pub fn get_field_definition_location(
        &self,
        position: Position,
        schema: &apollo_compiler::Schema,
        documents: &dashmap::DashMap<Url, DocumentState, ahash::RandomState>,
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
                    let mut curr = node;
                    while let Some(parent) = curr.parent() {
                        if parent.kind() == "field"
                            || parent.kind() == "argument"
                            || parent.kind() == "object_field"
                        {
                            let field_name = self.get_node_text(node, offset);

                            let mut search_type =
                                self.find_parent_type_for_node(parent, offset, schema);

                            if parent.kind() == "argument" {
                                // For arguments, we need the type that defines the field this argument belongs to,
                                // but then we look for the argument name within that field's definition.
                                // Actually, find_field_definition_in_schema can handle this if we pass the field name?
                                // No, find_field_definition_in_schema looks for a field in a type.
                                // If we have user(id: 1), we look for 'user' in Query, get its type, then look for 'id'?
                                // No, 'id' is defined ON 'user'.

                                if let Some(field_node) = parent.parent().and_then(|p| p.parent()) {
                                    if field_node.kind() == "field" {
                                        if let Some(parent_type) = self
                                            .find_parent_type_for_node(field_node, offset, schema)
                                        {
                                            let mut field_node_name = String::new();
                                            let mut f_walker = field_node.walk();
                                            for child in field_node.children(&mut f_walker) {
                                                if child.kind() == "name" {
                                                    field_node_name = self.get_node_text(child, offset);
                                                    break;
                                                }
                                            }

                                            for entry in documents.iter() {
                                                let doc = entry.value();
                                                if let Some(loc) = doc
                                                    .find_argument_definition_in_schema(
                                                        parent_type.name(),
                                                        &field_node_name,
                                                        &field_name,
                                                    )
                                                {
                                                    return Some(loc);
                                                }
                                            }
                                        }
                                    } else if field_node.kind() == "directive" {
                                        let mut dir_name = String::new();
                                        let mut f_walker = field_node.walk();
                                        for child in field_node.children(&mut f_walker) {
                                            if child.kind() == "name" {
                                                dir_name = self.get_node_text(child, offset);
                                                break;
                                            }
                                        }

                                        for entry in documents.iter() {
                                            let doc = entry.value();
                                            if let Some(loc) = doc
                                                .find_argument_definition_in_schema(
                                                    "Directive",
                                                    &dir_name,
                                                    &field_name,
                                                )
                                            {
                                                return Some(loc);
                                            }
                                        }
                                    }
                                }
                            } else if parent.kind() == "object_field" {
                                // For object fields in input types, find the expected input type
                                if let Some(obj_value) = parent.parent() {
                                    if let Some(val_node) = obj_value.parent() {
                                        search_type = self
                                            .find_expected_type_for_value(val_node, offset, schema);
                                    }
                                }
                            }

                            if let Some(pt) = search_type {
                                for entry in documents.iter() {
                                    let doc = entry.value();
                                    if let Some(loc) =
                                        doc.find_field_definition_in_schema(pt.name(), &field_name)
                                    {
                                        return Some(loc);
                                    }
                                }
                            }
                            break;
                        }
                        curr = parent;
                    }
                }
            }
        }
        None
    }

    pub fn find_field_definition_in_schema(
        &self,
        type_name: &str,
        field_name: &str,
    ) -> Option<Location> {
        let query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
        });

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
                    && n == type_name
                    && let Some(container) = container_node
                {
                    // Found the type, now look for the field inside its selection set or field definitions
                    let mut walker = container.walk();
                    for child in container.children(&mut walker) {
                        match child.kind() {
                            "fields_definition" => {
                                let mut f_walker = child.walk();
                                for f_child in child.children(&mut f_walker) {
                                    if f_child.kind() == "field_definition" {
                                        let mut fd_walker = f_child.walk();
                                        for fd_child in f_child.children(&mut fd_walker) {
                                            if fd_child.kind() == "name" {
                                                let f_name = self.get_node_text(fd_child, offset);
                                                if f_name == field_name {
                                                    return Some(Location {
                                                        uri: self.uri.clone(),
                                                        range: self.translate_to_file_range(
                                                            fd_child, offset,
                                                        ),
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            "field_definition" | "input_value_definition" => {
                                let mut f_walker = child.walk();
                                for f_child in child.children(&mut f_walker) {
                                    if f_child.kind() == "name" {
                                        let f_name = self.get_node_text(f_child, offset);
                                        if f_name == field_name {
                                            return Some(Location {
                                                uri: self.uri.clone(),
                                                range: self
                                                    .translate_to_file_range(f_child, offset),
                                            });
                                        }
                                    }
                                }
                            }
                            "enum_values_definition" => {
                                let mut f_walker = child.walk();
                                for f_child in child.children(&mut f_walker) {
                                    if f_child.kind() == "enum_value_definition" {
                                        let mut ev_walker = f_child.walk();
                                        for ev_child in f_child.children(&mut ev_walker) {
                                            if ev_child.kind() == "enum_value" {
                                                let mut v_walker = ev_child.walk();
                                                for v_child in ev_child.children(&mut v_walker) {
                                                    if v_child.kind() == "name" {
                                                        let f_name =
                                                            self.get_node_text(v_child, offset);
                                                        if f_name == field_name {
                                                            return Some(Location {
                                                                uri: self.uri.clone(),
                                                                range: self
                                                                    .translate_to_file_range(
                                                                        v_child, offset,
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
                            _ => {}
                        }
                    }
                }
            }
        }
        None
    }

    pub fn find_argument_definition_in_schema(
        &self,
        type_name: &str,
        field_name: &str,
        arg_name: &str,
    ) -> Option<Location> {
        let query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
        });

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
                let mut is_directive = false;

                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    if cap_name == "symbol.name" {
                        name = Some(self.get_node_text(cap.node, offset));
                    } else if cap_name == "symbol.container" {
                        container_node = Some(cap.node);
                        if cap.node.kind() == "directive_definition" {
                            is_directive = true;
                        }
                    }
                }

                if is_directive && type_name == "Directive" && name.as_deref() == Some(field_name) {
                    if let Some(container) = container_node {
                        let mut walker = container.walk();
                        for child in container.children(&mut walker) {
                            if child.kind() == "arguments_definition" {
                                let mut a_walker = child.walk();
                                for a_child in child.children(&mut a_walker) {
                                    if a_child.kind() == "input_value_definition" {
                                        let mut iv_walker = a_child.walk();
                                        for iv_child in a_child.children(&mut iv_walker) {
                                            if iv_child.kind() == "name" {
                                                if self.get_node_text(iv_child, offset) == arg_name
                                                {
                                                    return Some(Location {
                                                        uri: self.uri.clone(),
                                                        range: self.translate_to_file_range(
                                                            iv_child, offset,
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

                if let Some(n) = name
                    && n == type_name
                    && let Some(container) = container_node
                {
                    let mut walker = container.walk();
                    for child in container.children(&mut walker) {
                        if child.kind() == "fields_definition" {
                            let mut f_walker = child.walk();
                            for f_child in child.children(&mut f_walker) {
                                if f_child.kind() == "field_definition" {
                                    let mut fd_walker = f_child.walk();
                                    let mut found_field = false;
                                    for fd_child in f_child.children(&mut fd_walker) {
                                        if fd_child.kind() == "name" {
                                            if self.get_node_text(fd_child, offset) == field_name {
                                                found_field = true;
                                            }
                                        } else if fd_child.kind() == "arguments_definition"
                                            && found_field
                                        {
                                            let mut a_walker = fd_child.walk();
                                            for a_child in fd_child.children(&mut a_walker) {
                                                if a_child.kind() == "input_value_definition" {
                                                    let mut iv_walker = a_child.walk();
                                                    for iv_child in a_child.children(&mut iv_walker)
                                                    {
                                                        if iv_child.kind() == "name" {
                                                            if self.get_node_text(iv_child, offset)
                                                                == arg_name
                                                            {
                                                                return Some(Location {
                                                                    uri: self.uri.clone(),
                                                                    range: self
                                                                        .translate_to_file_range(
                                                                            iv_child, offset,
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
                    }
                }
            }
        }
        None
    }

    pub fn find_expected_type_for_value(
        &self,
        _node: tree_sitter::Node,
        _offset: usize,
        _schema: &apollo_compiler::Schema,
    ) -> Option<apollo_compiler::schema::ExtendedType> {
        // Placeholder for now as it's complex to implement correctly
        None
    }
}
