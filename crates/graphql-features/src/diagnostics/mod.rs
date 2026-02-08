use apollo_compiler::Schema;
use graphql_core::Config;
use graphql_core::document::DocumentState;
use graphql_core::queries::*;
use graphql_core::utils::{
    find_operation_range, mask_interpolations, push_duplicate_operation_diagnostic,
};
use lsp_types::*;
use tree_sitter::{Node, QueryCursor, StreamingIterator};

mod fragments;
mod operations;
mod selection_set;
mod values;

pub struct ValidationContext<'a> {
    pub schema: &'a apollo_compiler::validation::Valid<Schema>,
    pub all_fragments: &'a [crate::completion::FragmentCompletionInfo],
    pub used_fragments: Option<&'a ahash::AHashSet<String>>,
    pub used_variables: ahash::AHashSet<String>,
    pub defined_variables: ahash::AHashSet<String>,
    pub diagnostics: &'a mut Vec<Diagnostic>,
    pub config: Option<&'a Config>,
    pub include_ignored: bool,
    pub workspace_loaded: bool,
    pub is_operation: bool,
    pub selected_fields: ahash::AHashSet<String>,
    pub current_operation_type: Option<String>,
}

pub trait DocumentDiagnostics {
    fn get_semantic_diagnostics(
        &self,
        valid_schema: &apollo_compiler::validation::Valid<Schema>,
        all_fragments: &[crate::completion::FragmentCompletionInfo],
        used_fragments: Option<&ahash::AHashSet<String>>,
        config: Option<&Config>,
        verbose: bool,
        workspace_loaded: bool,
    ) -> Vec<Diagnostic>;

    fn validate_tree(&self, node: Node, offset: usize, ctx: &mut ValidationContext);

    fn is_deprecation_ignored(&self, reason: &str, config: Option<&Config>) -> bool;

    fn add_deprecation_diagnostic(
        &self,
        ctx: &mut ValidationContext,
        node: Node,
        offset: usize,
        message: String,
        reason: &str,
    );

    fn collect_gql_errors(
        &self,
        root: tree_sitter::Node,
        offset_byte: usize,
        diagnostics: &mut Vec<Diagnostic>,
    );
}

impl DocumentDiagnostics for DocumentState {
    fn get_semantic_diagnostics(
        &self,
        valid_schema: &apollo_compiler::validation::Valid<Schema>,
        all_fragments: &[crate::completion::FragmentCompletionInfo],
        used_fragments: Option<&ahash::AHashSet<String>>,
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

            // 2. Our manual schema validation (handles fragments across files)
            // We run this FIRST so we know what we handle ourselves.
            let mut manual_diagnostics = Vec::new();
            let mut ctx = ValidationContext {
                schema: valid_schema,
                all_fragments,
                used_fragments,
                used_variables: ahash::AHashSet::default(),
                defined_variables: ahash::AHashSet::default(),
                diagnostics: &mut manual_diagnostics,
                config,
                include_ignored: verbose,
                workspace_loaded,
                is_operation: false,
                selected_fields: ahash::AHashSet::default(),
                current_operation_type: None,
            };

            self.validate_tree(block.tree.root_node(), offset, &mut ctx);

            // 3. Validation diagnostics from apollo-compiler
            let block_text = self.get_node_text(block.tree.root_node(), offset);
            let masked = mask_interpolations(&block_text);

            let doc_res = apollo_compiler::executable::ExecutableDocument::parse(
                valid_schema,
                &masked,
                self.uri.as_str(),
            );

            let apollo_diagnostics: Vec<(
                String,
                Option<std::ops::Range<apollo_compiler::parser::LineColumn>>,
            )> = match doc_res {
                Ok(doc) => match doc.validate(valid_schema) {
                    Ok(_) => Vec::new(),
                    Err(errs) => errs
                        .errors
                        .iter()
                        .map(|e| (e.to_string(), e.line_column_range()))
                        .collect(),
                },
                Err(errs) => errs
                    .errors
                    .iter()
                    .map(|e| (e.to_string(), e.line_column_range()))
                    .collect(),
            };

            for (err_str, range_opt) in apollo_diagnostics {
                // Suppress apollo-compiler diagnostics that we handle ourselves
                // or that are redundant/confusing in our multi-file context.
                let is_duplicate = err_str.contains("defined multiple times")
                    || err_str.contains("is defined multiple times")
                    || err_str.contains("unused")
                    || err_str.contains("not defined")
                    || err_str.contains("is not defined")
                    || err_str.contains("not found on type")
                    || err_str.contains("does not have a field")
                    || err_str.contains("must be used in an operation")
                    || err_str.contains("fragment")
                        && err_str.contains("must be used in an operation")
                    || err_str.contains("must not contain an")
                    || err_str.contains("cannot select different fields into the same alias")
                    || err_str.contains("must not select different types using the same name")
                    || err_str.contains("conflicting field arguments")
                    || (err_str.contains("variable")
                        && err_str.contains("cannot be used for argument"))
                    // Suppress apollo-compiler messages about recursive / circular fragments
                    || err_str.contains("cannot reference itself")
                    || err_str.contains("references itself")
                    || err_str.contains("recursive fragment")
                    || err_str.contains("circular fragment")
                    || err_str.contains("circular reference");

                if !is_duplicate {
                    let range = if let Some(r) = range_opt {
                        Range {
                            start: Position::new(
                                r.start.line as u32 - 1,
                                r.start.column as u32 - 1,
                            ),
                            end: Position::new(r.end.line as u32 - 1, r.end.column as u32 - 1),
                        }
                    } else {
                        self.translate_to_file_range(block.tree.root_node(), offset)
                    };

                    diagnostics.push(Diagnostic {
                        range,
                        severity: Some(DiagnosticSeverity::ERROR),
                        message: format!("Apollo Validation Error: {}", err_str),
                        ..Default::default()
                    });
                }
            }

            // Append our manual diagnostics
            diagnostics.extend(manual_diagnostics);
        }

        // After validating tree nodes, optionally run cross-operation checks within this document.
        if let Some(cfg) = config
            && let Some(rules) = &cfg.rules
            && let Some(true) = rules.unique_operation_name
        {
            // Detect duplicate operation names within this document and report diagnostics.
            use std::collections::HashMap;
            let mut counts: HashMap<String, usize> = HashMap::new();
            for op in &self.operations {
                if let Some(name) = &op.name {
                    *counts.entry(name.clone()).or_insert(0) += 1;
                }
            }

            for (name, cnt) in counts.into_iter() {
                if cnt > 1
                    && let Some(range) = find_operation_range(self, &name)
                {
                    push_duplicate_operation_diagnostic(&mut diagnostics, range, &name, None);
                }
            }
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
                    operations::validate_operation(self, cap.node, offset, ctx);
                } else if capture_name == "fragment" {
                    fragments::validate_fragment(self, cap.node, offset, ctx, 0);
                }
            }
        }
    }

    fn is_deprecation_ignored(&self, reason: &str, config: Option<&Config>) -> bool {
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

    fn add_deprecation_diagnostic(
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
        root: tree_sitter::Node,
        offset_byte: usize,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
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
                stack.push(child);
            }
        }
    }
}
