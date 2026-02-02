use crate::document::DocumentState;
use crate::queries::*;
use apollo_compiler::Schema;
use tower_lsp::lsp_types::*;
use tree_sitter::{Node, QueryCursor, StreamingIterator};

mod fragments;
mod operations;
mod selection_set;
mod values;

impl DocumentState {
    pub fn get_semantic_diagnostics(
        &self,
        schema: &Schema,
        all_fragments: &[String],
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        let blocks = self.get_graphql_trees();

        for block in blocks {
            let offset = block.offset;
            // 1. Syntax errors
            self.collect_gql_errors(block.tree.root_node(), offset, &mut diagnostics);

            // 2. Schema validation
            self.validate_tree(
                block.tree.root_node(),
                offset,
                schema,
                all_fragments,
                &mut diagnostics,
            );
        }
        diagnostics
    }

    fn validate_tree(
        &self,
        node: Node,
        offset: usize,
        schema: &Schema,
        all_fragments: &[String],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let query = GQL_DIAGNOSTICS_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_DIAGNOSTICS_QUERY).unwrap()
        });

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, node, |n: Node| {
            let start = n.start_byte();
            let end = n.end_byte();
            self.rope
                .byte_slice((start + offset)..(end + offset))
                .chunks()
        });

        while let Some(m) = matches.next() {
            for cap in m.captures {
                let capture_name = query.capture_names()[cap.index as usize];
                if capture_name == "operation" {
                    self.validate_operation(cap.node, offset, schema, all_fragments, diagnostics);
                } else if capture_name == "fragment" {
                    self.validate_fragment(cap.node, offset, schema, all_fragments, diagnostics);
                }
            }
        }
    }

    fn collect_gql_errors(
        &self,
        node: tree_sitter::Node,
        offset_byte: usize,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        if node.is_error() || node.is_missing() {
            let range = self.translate_to_file_range(node, offset_byte);

            diagnostics.push(Diagnostic {
                range,
                severity: Some(DiagnosticSeverity::ERROR),
                message: format!("GraphQL Syntax Error: unexpected '{}'", node.kind()),
                ..Default::default()
            });
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.collect_gql_errors(child, offset_byte, diagnostics);
        }
    }
}
