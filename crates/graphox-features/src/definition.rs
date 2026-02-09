use ahash::RandomState;
use apollo_compiler::Schema;
use dashmap::DashMap;
use graphox_core::document::DocumentState;
use graphox_core::queries::*;
use lsp_types::*;
use std::sync::Arc;
use tree_sitter::{QueryCursor, StreamingIterator};
use url::Url;

use crate::shared::type_resolver::{self, SemanticSymbol};

pub trait DocumentDefinition {
    fn find_definition_in_tree(&self, name: &str) -> Option<Location>;
    fn find_type_definition_in_schema(
        &self,
        name: &str,
        query: &tree_sitter::Query,
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
    fn find_input_field_in_type_definition(
        &self,
        type_name: &str,
        field_name: &str,
        query: &tree_sitter::Query,
    ) -> Option<Location>;
    fn get_definition(
        &self,
        position: Position,
        schema: &Schema,
        documents: &DashMap<Url, Arc<DocumentState>, RandomState>,
        preferred_uris: &[Url],
    ) -> Option<Location>;
}

fn find_directive_in_document(doc: &DocumentState, directive_name: &str) -> Option<Location> {
    let query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
        let lang = tree_sitter_graphql::LANGUAGE.into();
        tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
    });

    let mut cursor = QueryCursor::new();
    for block in doc.get_graphql_trees() {
        let offset = block.offset;
        let mut matches = cursor.matches(query, block.tree.root_node(), |n: tree_sitter::Node| {
            let start = n.start_byte();
            let end = n.end_byte();
            doc.rope
                .byte_slice((start + offset)..(end + offset))
                .chunks()
        });

        while let Some(m) = matches.next() {
            for cap in m.captures {
                let cap_name = query.capture_names()[cap.index as usize];
                if cap_name == "symbol.name"
                    && doc.get_node_text(cap.node, offset) == directive_name
                    && let Some(parent) = cap.node.parent()
                    && parent.kind() == "directive_definition"
                {
                    return Some(Location {
                        uri: doc.uri.clone(),
                        range: doc.translate_to_file_range(cap.node, offset),
                    });
                }
            }
        }
    }
    None
}

impl DocumentDefinition for DocumentState {
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
                                    && let Some(ad_node) =
                                        self.find_child_by_kind(field_def, "arguments_definition")
                                {
                                    let mut ad_walker = ad_node.walk();
                                    for arg_def in ad_node.children(&mut ad_walker) {
                                        if arg_def.kind() == "input_value_definition"
                                            && let Some(a_name_node) =
                                                self.find_child_by_kind(arg_def, "name")
                                            && self.get_node_text(a_name_node, offset) == arg_name
                                        {
                                            return Some(Location {
                                                uri: self.uri.clone(),
                                                range: self
                                                    .translate_to_file_range(a_name_node, offset),
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
        None
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

    fn get_definition(
        &self,
        position: Position,
        schema: &Schema,
        documents: &DashMap<Url, Arc<DocumentState>, RandomState>,
        preferred_uris: &[Url],
    ) -> Option<Location> {
        let cursor_offset = self.position_to_byte(position);

        for block in self.get_graphql_trees() {
            let offset = block.offset;
            let root = block.tree.root_node();
            let tree_len = root.end_byte();

            if cursor_offset >= offset && cursor_offset < offset + tree_len {
                let local_byte = cursor_offset - offset;
                let node = root.descendant_for_byte_range(local_byte, local_byte)?;

                let symbol = type_resolver::resolve_symbol_at_node(
                    self,
                    node,
                    offset,
                    cursor_offset,
                    schema,
                )?;

                let symbol_query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
                    let lang = tree_sitter_graphql::LANGUAGE.into();
                    tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
                });

                match symbol {
                    SemanticSymbol::Field {
                        parent_type,
                        field_def,
                        alias: _alias,
                    } => {
                        let field_name = &field_def.name;
                        for p_uri in preferred_uris {
                            if let Some(doc) = documents.get(p_uri).map(|r| r.value().clone())
                                && let Some(loc) = doc.find_field_in_type_definition(
                                    parent_type.name(),
                                    field_name,
                                    symbol_query,
                                )
                            {
                                return Some(loc);
                            }
                        }
                        return self.find_field_in_type_definition(
                            parent_type.name(),
                            field_name,
                            symbol_query,
                        );
                    }
                    SemanticSymbol::Argument {
                        parent_type_name,
                        field_name,
                        arg_def,
                    } => {
                        let arg_name = &arg_def.name;
                        for p_uri in preferred_uris {
                            if let Some(doc) = documents.get(p_uri).map(|r| r.value().clone())
                                && let Some(loc) = doc.find_argument_in_field_definition(
                                    &parent_type_name,
                                    field_name.as_deref().unwrap_or(""),
                                    arg_name,
                                    symbol_query,
                                )
                            {
                                return Some(loc);
                            }
                        }
                        return self.find_argument_in_field_definition(
                            &parent_type_name,
                            field_name.as_deref().unwrap_or(""),
                            arg_name,
                            symbol_query,
                        );
                    }
                    SemanticSymbol::EnumValue { enum_name, val_def } => {
                        let enum_value_name = &val_def.value;
                        for p_uri in preferred_uris {
                            if let Some(doc) = documents.get(p_uri).map(|r| r.value().clone())
                                && let Some(loc) = doc.find_enum_value_in_schema(
                                    &enum_name,
                                    enum_value_name,
                                    symbol_query,
                                )
                            {
                                return Some(loc);
                            }
                        }
                        return self.find_enum_value_in_schema(
                            &enum_name,
                            enum_value_name,
                            symbol_query,
                        );
                    }
                    SemanticSymbol::InputObjectField {
                        parent_type,
                        field_def,
                    } => {
                        let field_name = &field_def.name;
                        for p_uri in preferred_uris {
                            if let Some(doc) = documents.get(p_uri).map(|r| r.value().clone())
                                && let Some(loc) = doc.find_input_field_in_type_definition(
                                    parent_type.name(),
                                    field_name,
                                    symbol_query,
                                )
                            {
                                return Some(loc);
                            }
                        }
                        return self.find_input_field_in_type_definition(
                            parent_type.name(),
                            field_name,
                            symbol_query,
                        );
                    }
                    SemanticSymbol::Type(ty) => {
                        let type_name = ty.name();
                        for p_uri in preferred_uris {
                            if let Some(doc) = documents.get(p_uri).map(|r| r.value().clone())
                                && let Some(loc) =
                                    doc.find_type_definition_in_schema(type_name, symbol_query)
                            {
                                return Some(loc);
                            }
                        }
                        return self.find_type_definition_in_schema(type_name, symbol_query);
                    }
                    SemanticSymbol::Directive { dir_def } => {
                        let directive_name = &dir_def.name;
                        for p_uri in preferred_uris {
                            if let Some(doc) = documents.get(p_uri).map(|r| r.value().clone())
                                && let Some(location) =
                                    find_directive_in_document(&doc, directive_name)
                            {
                                return Some(location);
                            }
                        }
                        return None;
                    }
                    SemanticSymbol::Variable { name, .. } => {
                        let location = self.find_definition_in_tree(&name)?;
                        return Some(location);
                    }
                    SemanticSymbol::TypeExtension { type_name, .. } => {
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
                    SemanticSymbol::Fragment { name, .. } => {
                        let location = self.find_definition_in_tree(&name)?;
                        return Some(location);
                    }
                    _ => {}
                }
            }
        }
        None
    }
}
