use super::ValidationContext;
use crate::document::DocumentState;
use apollo_compiler::schema::ExtendedType;
use tower_lsp::lsp_types::*;
use tree_sitter::Node;

impl DocumentState {
    pub(super) fn validate_fragment(&self, node: Node, offset: usize, ctx: &mut ValidationContext) {
        ctx.defined_variables.clear(); // Fragments don't have operation context variables

        let mut cursor = node.walk();
        let mut type_condition_node = None;
        let mut selection_set_node = None;
        let mut name_node = None;

        for child in node.children(&mut cursor) {
            if child.kind() == "fragment_name" {
                let mut name_cursor = child.walk();
                for name_child in child.children(&mut name_cursor) {
                    if name_child.kind() == "name" {
                        name_node = Some(name_child);
                    }
                }
            } else if child.kind() == "type_condition" {
                type_condition_node = Some(child);
            } else if child.kind() == "selection_set" {
                selection_set_node = Some(child);
            }
        }

        if let Some(name_node) = name_node {
            let name = self.get_node_text(name_node, offset);
            
            // 1. Unused fragment check
            let is_used = ctx.used_fragments.map(|u| u.contains(&name)).unwrap_or(true);
            if !is_used && ctx.workspace_loaded {
                ctx.diagnostics.push(Diagnostic {
                    range: self.translate_to_file_range(node, offset),
                    severity: Some(DiagnosticSeverity::WARNING),
                    message: format!("Unused fragment: {}", name),
                    code: Some(NumberOrString::String("unused_fragment".to_string())),
                    tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                    ..Default::default()
                });
            }

            // 2. Collision and shadowing checks
            let current_frag_def = self.fragments.iter().find(|f| f.name == name);
            if let Some(current_frag) = current_frag_def {
                let current_is_public = current_frag.is_public;
                let current_package_root = self.package_root.as_ref();

                for other in ctx.all_fragments {
                    if other.name == name && other.uri != self.uri {
                        let other_package_root = &other.package_root;
                        
                        if current_is_public && other.is_public {
                            ctx.diagnostics.push(Diagnostic {
                                range: self.translate_to_file_range(name_node, offset),
                                severity: Some(DiagnosticSeverity::ERROR),
                                message: format!("Duplicate public fragment name: '{}'. Public fragments must have unique names across the workspace.", name),
                                ..Default::default()
                            });
                            break;
                        } else if !current_is_public && !other.is_public && current_package_root == other_package_root.as_ref() {
                            ctx.diagnostics.push(Diagnostic {
                                range: self.translate_to_file_range(name_node, offset),
                                severity: Some(DiagnosticSeverity::ERROR),
                                message: format!("Duplicate fragment name: '{}' in the same project.", name),
                                ..Default::default()
                            });
                            break;
                        } else if !current_is_public && other.is_public {
                            ctx.diagnostics.push(Diagnostic {
                                range: self.translate_to_file_range(name_node, offset),
                                severity: Some(DiagnosticSeverity::HINT),
                                message: format!("Private fragment '{}' shadows a public fragment defined in {}.", name, other.uri),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
        }

        if let Some(type_cond) = type_condition_node {
            let mut tc_cursor = type_cond.walk();
            for tc_child in type_cond.children(&mut tc_cursor) {
                if tc_child.kind() == "named_type" {
                    let mut nt_cursor = tc_child.walk();
                    for nt_child in tc_child.children(&mut nt_cursor) {
                        if nt_child.kind() == "name" {
                            let type_name = self.get_node_text(nt_child, offset);
                            if let Some(type_def) = ctx.schema.types.get(type_name.as_str())
                                && let Some(sel_set) = selection_set_node
                            {
                                self.validate_selection_set(sel_set, offset, type_def, ctx);
                            }
                        }
                    }
                }
            }
        }
    }

    pub(super) fn validate_inline_fragment(
        &self,
        node: Node,
        offset: usize,
        parent_type: &ExtendedType,
        ctx: &mut ValidationContext,
    ) {
        let mut cursor = node.walk();
        let mut type_condition_node = None;
        let mut selection_set_node = None;

        for child in node.children(&mut cursor) {
            if child.kind() == "type_condition" {
                type_condition_node = Some(child);
            } else if child.kind() == "selection_set" {
                selection_set_node = Some(child);
            }
        }

        let target_type = if let Some(type_cond) = type_condition_node {
            let mut tc_cursor = type_cond.walk();
            let mut found_type = None;
            for tc_child in type_cond.children(&mut tc_cursor) {
                if tc_child.kind() == "named_type" {
                    let mut nt_cursor = tc_child.walk();
                    for nt_child in tc_child.children(&mut nt_cursor) {
                        if nt_child.kind() == "name" {
                            let type_name = self.get_node_text(nt_child, offset);
                            found_type = ctx.schema.types.get(type_name.as_str());
                            break;
                        }
                    }
                }
            }
            found_type
        } else {
            Some(parent_type)
        };

        if let Some(t_type) = target_type
            && let Some(sel_set) = selection_set_node
        {
            self.validate_selection_set(sel_set, offset, t_type, ctx);
        }
    }

    pub(super) fn validate_fragment_spread(
        &self,
        node: Node,
        offset: usize,
        ctx: &mut ValidationContext,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "fragment_name" {
                let mut name_cursor = child.walk();
                for name_child in child.children(&mut name_cursor) {
                    if name_child.kind() == "name" {
                        let name = self.get_node_text(name_child, offset);
                        let mut visited = fnv::FnvHashSet::default();
                        let exists = self.mark_used_variables_recursive(
                            &name,
                            ctx,
                            &mut visited,
                            name_child,
                            offset,
                        );

                        if !exists && ctx.workspace_loaded {
                            ctx.diagnostics.push(Diagnostic {
                                range: self.translate_to_file_range(name_child, offset),
                                severity: Some(DiagnosticSeverity::ERROR),
                                message: format!("Unknown fragment: {}", name),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
        }
    }

    fn mark_used_variables_recursive(
        &self,
        name: &str,
        ctx: &mut ValidationContext,
        visited: &mut fnv::FnvHashSet<String>,
        trigger_node: Node,
        offset: usize,
    ) -> bool {
        if !visited.insert(name.to_string()) {
            return true;
        }

        let mut used_variables = None;
        let mut used_fragments = None;
        let mut exists = false;

        if let Some(f) = ctx.all_fragments.iter().find(|f| f.name == name) {
            exists = true;
            used_variables = Some(&f.used_variables);
            used_fragments = Some(&f.used_fragments);
        } else if let Some(f) = self.fragments.iter().find(|f| f.name == name) {
            exists = true;
            used_variables = Some(&f.used_variables);
            used_fragments = Some(&f.used_fragments);
        }

        if let Some(vars) = used_variables {
            for var in vars {
                ctx.used_variables.insert(var.clone());

                // Only report undefined variables if we are in an operation context
                if !ctx.defined_variables.is_empty() {
                    if !ctx.defined_variables.contains(var) {
                        ctx.diagnostics.push(Diagnostic {
                            range: self.translate_to_file_range(trigger_node, offset),
                            severity: Some(DiagnosticSeverity::ERROR),
                            message: format!(
                                "Undefined variable: ${} (required by fragment '{}')",
                                var, name
                            ),
                            code: Some(NumberOrString::String("undefined_variable".to_string())),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        if let Some(frags) = used_fragments {
            for frag in frags {
                self.mark_used_variables_recursive(frag, ctx, visited, trigger_node, offset);
            }
        }

        exists
    }
}
