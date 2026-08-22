use ahash::RandomState;
use apollo_compiler::Schema;
use dashmap::DashMap;
use graphox_core::document::DocumentState;
use graphox_core::queries::*;
use ls_types::*;
use std::sync::Arc;
use tree_sitter::{QueryCursor, StreamingIterator};
use ls_types::Uri;

use crate::shared::type_resolver::{self, SemanticSymbol};

pub trait DocumentDefinition {
    fn find_definition_in_tree(&self, name: &str) -> Option<Location>;
    fn find_fragment_definition_in_tree(&self, name: &str) -> Option<Location>;
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
        field_name: Option<&str>,
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
        subgraphs: Option<&[graphox_core::schema::SubgraphInfo]>,
        documents: &DashMap<Uri, Arc<DocumentState>, RandomState>,
        preferred_uris: &[Uri],
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
                    && doc.get_node_text(cap.node, offset).trim() == directive_name
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
                    if self.get_node_text(cap.node, offset).trim() == name {
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

    fn find_fragment_definition_in_tree(&self, name: &str) -> Option<Location> {
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
                    if self.get_node_text(cap.node, offset).trim() != name {
                        continue;
                    }

                    let Some(parent) = cap.node.parent() else {
                        continue;
                    };
                    if parent.kind() != "fragment_name" {
                        continue;
                    }
                    let Some(grandparent) = parent.parent() else {
                        continue;
                    };
                    if grandparent.kind() != "fragment_definition" {
                        continue;
                    }

                    return Some(Location {
                        uri: self.uri.clone(),
                        range: self.translate_to_file_range(cap.node, offset),
                    });
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
                    if cap_name == "symbol.name"
                        && self.get_node_text(cap.node, offset).trim() == name
                    {
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
                    if cap_name == "enum_value.name"
                        && self.get_node_text(cap.node, offset).trim() == enum_value_name
                    {
                        // Check parent container
                        let mut curr = cap.node.parent();
                        while let Some(node) = curr {
                            if (node.kind() == "enum_type_definition"
                                || node.kind() == "enum_type_extension")
                                && let Some(name_node) = self.find_child_by_kind(node, "name")
                                && self.get_node_text(name_node, offset).trim() == enum_name
                            {
                                return Some(Location {
                                    uri: self.uri.clone(),
                                    range: self.translate_to_file_range(cap.node, offset),
                                });
                            }
                            curr = node.parent();
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
                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    if cap_name == "field.name"
                        && self.get_node_text(cap.node, offset).trim() == field_name
                    {
                        // Check parent container by walking up ancestors
                        let mut curr = cap.node.parent();
                        while let Some(node) = curr {
                            if (node.kind() == "object_type_definition"
                                || node.kind() == "interface_type_definition"
                                || node.kind() == "object_type_extension"
                                || node.kind() == "interface_type_extension")
                                && let Some(name_node) = self.find_child_by_kind(node, "name")
                                && self.get_node_text(name_node, offset).trim() == type_name
                            {
                                return Some(Location {
                                    uri: self.uri.clone(),
                                    range: self.translate_to_file_range(cap.node, offset),
                                });
                            }
                            curr = node.parent();
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
        field_name: Option<&str>,
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
                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    if cap_name == "argument.name"
                        && self.get_node_text(cap.node, offset).trim() == arg_name
                    {
                        // Check parent field/directive and type
                        let mut curr = cap.node.parent();
                        let mut found_parent = false;
                        while let Some(node) = curr {
                            if let Some(f_name) = field_name {
                                if node.kind() == "field_definition" {
                                    if let Some(fn_node) = self.find_child_by_kind(node, "name")
                                        && self.get_node_text(fn_node, offset).trim() == f_name
                                    {
                                        found_parent = true;
                                    }
                                } else if (node.kind() == "object_type_definition"
                                    || node.kind() == "object_type_extension"
                                    || node.kind() == "interface_type_definition"
                                    || node.kind() == "interface_type_extension")
                                    && found_parent
                                    && let Some(tn_node) = self.find_child_by_kind(node, "name")
                                    && self.get_node_text(tn_node, offset).trim() == type_name
                                {
                                    return Some(Location {
                                        uri: self.uri.clone(),
                                        range: self.translate_to_file_range(cap.node, offset),
                                    });
                                }
                            } else {
                                // Directive case
                                if node.kind() == "directive_definition"
                                    && let Some(dn_node) = self.find_child_by_kind(node, "name")
                                    && self.get_node_text(dn_node, offset).trim() == type_name
                                {
                                    return Some(Location {
                                        uri: self.uri.clone(),
                                        range: self.translate_to_file_range(cap.node, offset),
                                    });
                                }
                            }
                            curr = node.parent();
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
                for cap in m.captures {
                    let cap_name = query.capture_names()[cap.index as usize];
                    if cap_name == "argument.name"
                        && self.get_node_text(cap.node, offset).trim() == field_name
                    {
                        // Check parent input object
                        let mut curr = cap.node.parent();
                        while let Some(node) = curr {
                            if (node.kind() == "input_object_type_definition"
                                || node.kind() == "input_object_type_extension")
                                && let Some(name_node) = self.find_child_by_kind(node, "name")
                                && self.get_node_text(name_node, offset).trim() == type_name
                            {
                                return Some(Location {
                                    uri: self.uri.clone(),
                                    range: self.translate_to_file_range(cap.node, offset),
                                });
                            }
                            curr = node.parent();
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
        _subgraphs: Option<&[graphox_core::schema::SubgraphInfo]>,
        documents: &DashMap<Uri, Arc<DocumentState>, RandomState>,
        preferred_uris: &[Uri],
    ) -> Option<Location> {
        let symbol_query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
        });

        // Try to resolve symbol first
        let byte_offset = self.position_to_byte(position);
        if let Some((node, offset)) = self.find_node_at_position(position)
            && let Some(SemanticSymbol::Fragment { name, .. }) =
                type_resolver::resolve_fragment_spread_at_node(self, node, offset, byte_offset)
        {
            if let Some(loc) = self.find_fragment_definition_in_tree(&name) {
                return Some(loc);
            }

            for p_uri in preferred_uris {
                if p_uri == &self.uri {
                    continue;
                }
                let doc_arc = load_document_for_uri(p_uri, documents, &self.position_encoding);
                if let Some(doc) = doc_arc
                    && let Some(loc) = doc.find_fragment_definition_in_tree(&name)
                {
                    return Some(loc);
                }
            }

            return None;
        }

        if let Some((node, offset)) = self.find_node_at_position(position)
            && let Some(symbol) =
                type_resolver::resolve_symbol_at_node(self, node, offset, byte_offset, schema)
        {
            match symbol {
                SemanticSymbol::Field {
                    parent_type,
                    field_def,
                    ..
                } => {
                    let type_name = parent_type.name();
                    let field_name = &field_def.name;
                    for p_uri in preferred_uris {
                        let doc_arc =
                            load_document_for_uri(p_uri, documents, &self.position_encoding);
                        if let Some(doc) = doc_arc
                            && let Some(loc) = doc.find_field_in_type_definition(
                                type_name,
                                field_name,
                                symbol_query,
                            )
                        {
                            return Some(loc);
                        }
                    }
                }
                SemanticSymbol::Argument {
                    parent_type,
                    field_name,
                    arg_def,
                } => {
                    let arg_name = &arg_def.name;
                    for p_uri in preferred_uris {
                        let doc_arc =
                            load_document_for_uri(p_uri, documents, &self.position_encoding);
                        if let Some(doc) = doc_arc
                            && let Some(loc) = doc.find_argument_in_field_definition(
                                &parent_type,
                                field_name.as_deref(),
                                arg_name,
                                symbol_query,
                            )
                        {
                            return Some(loc);
                        }
                    }
                }
                SemanticSymbol::Directive { dir_def } => {
                    let dir_name = &dir_def.name;
                    for p_uri in preferred_uris {
                        let doc_arc =
                            load_document_for_uri(p_uri, documents, &self.position_encoding);
                        if let Some(doc) = doc_arc
                            && let Some(loc) = find_directive_in_document(&doc, dir_name)
                        {
                            return Some(loc);
                        }
                    }
                }
                SemanticSymbol::Type(ty) => {
                    let type_name = ty.name();
                    for p_uri in preferred_uris {
                        let doc_arc =
                            load_document_for_uri(p_uri, documents, &self.position_encoding);
                        if let Some(doc) = doc_arc
                            && let Some(loc) =
                                doc.find_type_definition_in_schema(type_name, symbol_query)
                        {
                            return Some(loc);
                        }
                    }
                }
                SemanticSymbol::Fragment { name, .. } => {
                    if let Some(loc) = self.find_fragment_definition_in_tree(&name) {
                        return Some(loc);
                    }

                    for p_uri in preferred_uris {
                        if p_uri == &self.uri {
                            continue;
                        }
                        let doc_arc =
                            load_document_for_uri(p_uri, documents, &self.position_encoding);
                        if let Some(doc) = doc_arc
                            && let Some(loc) = doc.find_fragment_definition_in_tree(&name)
                        {
                            return Some(loc);
                        }
                    }
                }
                SemanticSymbol::EnumValue {
                    enum_name, val_def, ..
                } => {
                    let val_name = &val_def.value;
                    for p_uri in preferred_uris {
                        let doc_arc =
                            load_document_for_uri(p_uri, documents, &self.position_encoding);
                        if let Some(doc) = doc_arc
                            && let Some(loc) =
                                doc.find_enum_value_in_schema(&enum_name, val_name, symbol_query)
                        {
                            return Some(loc);
                        }
                    }
                }
                SemanticSymbol::InputObjectField {
                    parent_type,
                    field_def,
                } => {
                    let type_name = parent_type.name();
                    let field_name = &field_def.name;
                    for p_uri in preferred_uris {
                        let doc_arc =
                            load_document_for_uri(p_uri, documents, &self.position_encoding);
                        if let Some(doc) = doc_arc
                            && let Some(loc) = doc.find_input_field_in_type_definition(
                                type_name,
                                field_name,
                                symbol_query,
                            )
                        {
                            return Some(loc);
                        }
                    }
                }
                SemanticSymbol::Variable { name, .. } => {
                    use crate::references::DocumentReferences;
                    if let Some(loc) = self.find_variable_declaration(&name, position) {
                        return Some(loc);
                    }
                    for p_uri in preferred_uris {
                        if p_uri == &self.uri {
                            continue;
                        }
                        let doc_arc =
                            load_document_for_uri(p_uri, documents, &self.position_encoding);
                        if let Some(doc) = doc_arc
                            && let Some(loc) =
                                doc.find_variable_declaration(&name, Position::new(0, 0))
                        {
                            return Some(loc);
                        }
                    }
                }
                _ => {}
            }
        }

        // 2. Fallback to name-based lookup in tree if symbol resolution failed or didn't find anything
        if let Some(name) = self.get_symbol_at_position(position) {
            let name = name.trim();
            // Check current file first
            if let Some(loc) = self.find_definition_in_tree(name) {
                return Some(loc);
            }

            // Check preferred schema URIs
            for p_uri in preferred_uris {
                if p_uri == &self.uri {
                    continue;
                }
                let doc_arc = load_document_for_uri(p_uri, documents, &self.position_encoding);

                if let Some(doc) = doc_arc
                    && let Some(loc) = doc.find_definition_in_tree(name)
                {
                    return Some(loc);
                }
            }
        }

        None
    }
}

fn load_document_for_uri(
    uri: &Uri,
    documents: &DashMap<Uri, Arc<DocumentState>, RandomState>,
    position_encoding: &PositionEncodingKind,
) -> Option<Arc<DocumentState>> {
    if let Some(doc) = documents.get(uri).map(|r| r.value().clone()) {
        Some(doc)
    } else if let Some(path) = uri.to_file_path()
        && let Ok(content) = std::fs::read_to_string(&path)
    {
        Some(Arc::new(DocumentState::new_from_thread_local(
            uri.clone(),
            &content,
            position_encoding.clone(),
        )))
    } else {
        None
    }
}
