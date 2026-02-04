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

    pub fn find_containing_operation_node(&self, position: Position) -> Option<(tree_sitter::Node<'_>, usize)> {
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

    pub fn find_variable_definition(&self, symbol_name: &str, position: Position) -> Option<Location> {
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
                        if parent.kind() == "field" {
                            let parent_type = self.find_parent_type_for_node(parent, offset, schema)?;
                            let field_name = self.get_node_text(node, offset);

                            for entry in documents.iter() {
                                let doc = entry.value();
                                if let Some(loc) =
                                    doc.find_field_definition_in_schema(parent_type.name(), &field_name)
                                {
                                    return Some(loc);
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
                                                        range: self.translate_to_file_range(fd_child, offset),
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
                                                range: self.translate_to_file_range(f_child, offset),
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
                                                        let f_name = self.get_node_text(v_child, offset);
                                                        if f_name == field_name {
                                                            return Some(Location {
                                                                uri: self.uri.clone(),
                                                                range: self.translate_to_file_range(v_child, offset),
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
}
