use crate::Config;
use crate::document::DocumentState;
use crate::queries::*;
use crate::utils::mask_interpolations;
use apollo_compiler::Schema;
use tower_lsp::lsp_types::*;
use tree_sitter::{Node, QueryCursor, StreamingIterator};

mod fragments;
mod operations;
mod selection_set;
mod values;

pub(super) struct ValidationContext<'a> {
    pub schema: &'a Schema,
    pub all_fragments: &'a [crate::features::completion::FragmentCompletionInfo],
    pub used_fragments: Option<&'a fnv::FnvHashSet<String>>,
    pub used_variables: fnv::FnvHashSet<String>,
    pub diagnostics: &'a mut Vec<Diagnostic>,
    pub config: Option<&'a Config>,
    pub include_ignored: bool,
    pub workspace_loaded: bool,
}

impl DocumentState {
    pub fn get_semantic_diagnostics(
        &self,
        schema: &Schema,
        all_fragments: &[crate::features::completion::FragmentCompletionInfo],
        used_fragments: Option<&fnv::FnvHashSet<String>>,
        config: Option<&Config>,
        verbose: bool,
        workspace_loaded: bool,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();

        let blocks = self.get_graphql_trees();

        for block in blocks {
            let offset = block.offset;
            // 1. Syntax errors from Tree-sitter
            self.collect_gql_errors(block.tree.root_node(), offset, &mut diagnostics);

            // 2. Parser/Validation errors from apollo-compiler
            let block_text = self.get_node_text(block.tree.root_node(), offset);
            let masked = mask_interpolations(&block_text);

            if let Ok(valid_schema) = schema.clone().validate() {
                let doc_res = apollo_compiler::executable::ExecutableDocument::parse(
                    &valid_schema,
                    &masked,
                    "doc.graphql",
                );
                if let Err(with_errors) = doc_res {
                    // Only report parser errors if they are not about schema definitions in executable docs
                    // (because the tool currently scans schema files too)
                    let reportable_errors: Vec<_> = with_errors
                        .errors
                        .iter()
                        .filter(|e| !e.to_string().contains("must not contain"))
                        .collect();

                    if !reportable_errors.is_empty() {
                        diagnostics.push(Diagnostic {
                            range: self.translate_to_file_range(block.tree.root_node(), offset),
                            severity: Some(DiagnosticSeverity::ERROR),
                            message: format!("GraphQL Parse Error: {}", with_errors.errors),
                            ..Default::default()
                        });
                    }
                }
            }

            // 3. Our manual schema validation (handles fragments across files)
            let mut ctx = ValidationContext {
                schema,
                all_fragments,
                used_fragments,
                used_variables: fnv::FnvHashSet::default(),
                diagnostics: &mut diagnostics,
                config,
                include_ignored: verbose,
                workspace_loaded,
            };

            self.validate_tree(block.tree.root_node(), offset, &mut ctx);
        }
        diagnostics
    }

    fn validate_tree(&self, node: Node, offset: usize, ctx: &mut ValidationContext) {
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
                    self.validate_operation(cap.node, offset, ctx);
                } else if capture_name == "fragment" {
                    self.validate_fragment(cap.node, offset, ctx);
                }
            }
        }
    }

    pub(super) fn is_deprecation_ignored(&self, reason: &str, config: Option<&Config>) -> bool {
        if let Some(cfg) = config
            && let Some(patterns) = &cfg.ignore_deprecations
        {
            for p in patterns {
                if let Ok(re) = regex::Regex::new(p)
                    && re.is_match(reason)
                {
                    return true;
                }
            }
        }
        false
    }

    pub(super) fn add_deprecation_diagnostic(
        &self,
        ctx: &mut ValidationContext,
        node: Node,
        offset: usize,
        message: String,
        reason: &str,
    ) {
        if !self.is_deprecation_ignored(reason, ctx.config) {
            ctx.diagnostics.push(Diagnostic {
                range: self.translate_to_file_range(node, offset),
                severity: Some(DiagnosticSeverity::WARNING),
                message,
                ..Default::default()
            });
        } else if ctx.include_ignored {
            ctx.diagnostics.push(Diagnostic {
                range: self.translate_to_file_range(node, offset),
                severity: Some(DiagnosticSeverity::INFORMATION),
                message: format!("[Ignored] {}", message),
                ..Default::default()
            });
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
