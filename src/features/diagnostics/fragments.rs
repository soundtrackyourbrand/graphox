use super::ValidationContext;
use crate::document::DocumentState;
use apollo_compiler::schema::ExtendedType;
use tower_lsp::lsp_types::*;
use tree_sitter::Node;

impl DocumentState {
    pub(super) fn validate_fragment(
        &self,
        node: Node,
        offset: usize,
        ctx: &mut ValidationContext,
        depth: usize,
    ) {
        ctx.is_operation = false;
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
            } else if child.kind() == "directives" || child.kind() == "directive" {
                self.validate_directives(child, offset, ctx);
            }
        }

        if let Some(name_node) = name_node {
            let name = self.get_node_text(name_node, offset);
            let current_frag_def = self.fragments.iter().find(|f| f.name == name);
            let is_type_only = current_frag_def.map(|f| f.is_type_only).unwrap_or(false);

            // 1. Unused fragment check
            let is_used = ctx
                .used_fragments
                .map(|u| u.contains(&name))
                .unwrap_or(true);

            if !is_used && ctx.workspace_loaded && !is_type_only {
                ctx.diagnostics.push(Diagnostic {
                    range: self.translate_to_file_range(name_node, offset),
                    severity: Some(DiagnosticSeverity::WARNING),
                    message: format!("Unused fragment: {}", name),
                    code: Some(NumberOrString::String("unused_fragment".to_string())),
                    tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                    ..Default::default()
                });
            }

            // 2. Used but marked as @type_only
            if is_used && is_type_only {
                let mut directive_range = self.translate_to_file_range(node, offset);

                // Try to find the specific directive node for a better range and easier removal
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "directives" {
                        let mut dir_cursor = child.walk();
                        for dir_child in child.children(&mut dir_cursor) {
                            if dir_child.kind() == "directive" {
                                let dir_text = self.get_node_text(dir_child, offset);
                                if dir_text.contains("@type_only") {
                                    directive_range =
                                        self.translate_to_file_range(dir_child, offset);
                                    break;
                                }
                            }
                        }
                    }
                }

                ctx.diagnostics.push(Diagnostic {
                    range: directive_range,
                    severity: Some(DiagnosticSeverity::WARNING),
                    message: format!("Fragment '{}' is used but marked with @type_only. Remove @type_only to resolve this warning.", name),
                    code: Some(NumberOrString::String("type_only_used".to_string())),
                    ..Default::default()
                });
            }

            // 3. Collision and shadowing checks
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
                        } else if !current_is_public
                            && !other.is_public
                            && current_package_root == other_package_root.as_ref()
                        {
                            ctx.diagnostics.push(Diagnostic {
                                range: self.translate_to_file_range(name_node, offset),
                                severity: Some(DiagnosticSeverity::ERROR),
                                message: format!(
                                    "Duplicate fragment name: '{}' in the same project.",
                                    name
                                ),
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
                                self.validate_selection_set(
                                    sel_set,
                                    offset,
                                    type_def,
                                    ctx,
                                    depth + 1,
                                );
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
        depth: usize,
    ) {
        if depth > 100 {
            return;
        }
        let mut cursor = node.walk();
        let mut type_condition_node = None;
        let mut selection_set_node = None;

        for child in node.children(&mut cursor) {
            if child.kind() == "type_condition" {
                type_condition_node = Some(child);
            } else if child.kind() == "selection_set" {
                selection_set_node = Some(child);
            } else if child.kind() == "directives" || child.kind() == "directive" {
                self.validate_directives(child, offset, ctx);
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
            self.validate_selection_set(sel_set, offset, t_type, ctx, depth + 1);
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
            } else if child.kind() == "directives" || child.kind() == "directive" {
                self.validate_directives(child, offset, ctx);
            }
        }
    }

    fn mark_used_variables_recursive(
        &self,
        initial_name: &str,
        ctx: &mut ValidationContext,
        visited: &mut fnv::FnvHashSet<String>,
        trigger_node: Node,
        offset: usize,
    ) -> bool {
        // Use a DFS traversal to detect cycles and collect used variables.
        fn dfs(
            this: &DocumentState,
            name: &str,
            ctx: &mut ValidationContext,
            visited: &mut fnv::FnvHashSet<String>,
            path: &mut Vec<String>,
            trigger_node: Node,
            offset: usize,
        ) -> bool {
            // If this fragment is already on the current path, we found a cycle.
            if path.contains(&name.to_string()) {
                // Build the cycle using only the relevant slice of the path
                let start_idx = path.iter().position(|p| p == name).unwrap_or(0);
                let mut cycle_names: Vec<String> = path[start_idx..].to_vec();
                // append the repeating fragment at the end to close the cycle
                cycle_names.push(name.to_string());

                // Build parts with URIs when available
                let mut cycle_parts: Vec<String> = cycle_names
                    .iter()
                    .map(|n| {
                        let mut part = n.clone();
                        if let Some(fmeta) = ctx.all_fragments.iter().find(|f| f.name == *n) {
                            part.push_str(" (");
                            part.push_str(fmeta.uri.path());
                            part.push(')');
                        } else if this.fragments.iter().any(|f| f.name == *n) {
                            part.push_str(" (");
                            part.push_str(this.uri.path());
                            part.push(')');
                        }
                        part
                    })
                    .collect();

                // Canonicalize the cycle rotation for deterministic messages.
                // Find the minimal fragment name (lexicographically) among the unique cycle nodes
                if cycle_parts.len() > 2 {
                    // exclude the final repeated element when choosing rotation
                    let unique_len = cycle_parts.len() - 1;
                    let mut min_idx = 0usize;
                    for i in 0..unique_len {
                        // compare by fragment name (before the first space which precedes the URI)
                        let a = cycle_parts[i].split(' ').next().unwrap_or(&cycle_parts[i]);
                        let b = cycle_parts[min_idx]
                            .split(' ')
                            .next()
                            .unwrap_or(&cycle_parts[min_idx]);
                        if a < b {
                            min_idx = i;
                        }
                    }

                    if min_idx > 0 {
                        // rotate the unique portion
                        let mut rotated: Vec<String> = cycle_parts[0..unique_len].to_vec();
                        rotated.rotate_left(min_idx);
                        // append the first element again to close the cycle
                        rotated.push(rotated[0].clone());
                        cycle_parts = rotated;
                    }
                }

                let cycle = cycle_parts.join(" -> ");

                ctx.diagnostics.push(Diagnostic {
                    range: this.translate_to_file_range(trigger_node, offset),
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: format!("Circular fragment reference: {}", cycle),
                    code: Some(NumberOrString::String("circular_fragment".to_string())),
                    ..Default::default()
                });
                return true;
            }

            // Avoid reprocessing fragments we've fully visited
            if visited.contains(name) {
                return true;
            }

            // Find fragment definition either in workspace fragments or this document
            let mut fragment_exists = false;
            let mut used_variables = None;
            let mut used_fragments = None;
            if let Some(f) = ctx.all_fragments.iter().find(|f| f.name == name) {
                fragment_exists = true;
                used_variables = Some(&f.used_variables);
                used_fragments = Some(&f.used_fragments);
            } else if let Some(f) = this.fragments.iter().find(|f| f.name == name) {
                fragment_exists = true;
                used_variables = Some(&f.used_variables);
                used_fragments = Some(&f.used_fragments);
            }

            if !fragment_exists {
                return false;
            }

            // mark as on current path
            path.push(name.to_string());

            // collect variables
            if let Some(vars) = used_variables {
                for var in vars {
                    ctx.used_variables.insert(var.clone());

                    if !ctx.defined_variables.is_empty() && !ctx.defined_variables.contains(var) {
                        ctx.diagnostics.push(Diagnostic {
                            range: this.translate_to_file_range(trigger_node, offset),
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

            // Recurse into used fragments
            if let Some(frags) = used_fragments {
                for frag in frags {
                    // Recurse - if any recursion reports cycle we propagate
                    let _ = dfs(this, frag, ctx, visited, path, trigger_node, offset);
                }
            }

            // finished exploring this node
            path.pop();
            visited.insert(name.to_string());
            true
        }

        let mut path = Vec::new();
        let local_visited = visited;
        // return whether initial fragment exists
        dfs(
            self,
            initial_name,
            ctx,
            local_visited,
            &mut path,
            trigger_node,
            offset,
        )
    }
}
