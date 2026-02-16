use super::ValidationContext;
use apollo_compiler::schema::ExtendedType;
use graphox_core::document::DocumentState;
use graphox_core::queries::{GQL_SYMBOL_QUERY, GQL_SYMBOL_QUERY_CACHE};
use lsp_types::*;
use tree_sitter::{Node, StreamingIterator};

pub(super) fn validate_fragment(
    this: &DocumentState,
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
            crate::diagnostics::values::validate_directives(this, child, offset, ctx);
        }
    }

    if let Some(name_node) = name_node {
        let name = this.get_node_text(name_node, offset);
        let current_frag_def = this.fragments.iter().find(|f| f.name.as_ref() == name);
        let is_type_only = current_frag_def.map(|f| f.is_type_only).unwrap_or(false);

        // 1. Unused fragment check
        let is_used = ctx
            .used_fragments
            .map(|u| u.contains(name.as_str()))
            .unwrap_or(true);

        let no_unused_fragments_enabled = ctx
            .config
            .as_ref()
            .map(|c| c.rules().no_unused_fragments())
            .unwrap_or(false);

        if !is_used && ctx.workspace_loaded && !is_type_only && no_unused_fragments_enabled {
            ctx.diagnostics.push(Diagnostic {
                range: this.translate_to_file_range(name_node, offset),
                severity: Some(DiagnosticSeverity::WARNING),
                message: format!("Unused fragment: {}", name),
                code: Some(NumberOrString::String("unused_fragment".to_string())),
                tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                ..Default::default()
            });
        }

        // 2. Used but marked as @type_only
        if is_used && is_type_only {
            let mut directive_range = this.translate_to_file_range(node, offset);

            // Try to find the specific directive node for a better range and easier removal
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "directives" {
                    let mut dir_cursor = child.walk();
                    for dir_child in child.children(&mut dir_cursor) {
                        if dir_child.kind() == "directive" {
                            let dir_text = this.get_node_text(dir_child, offset);
                            if dir_text.contains("@type_only") {
                                directive_range = this.translate_to_file_range(dir_child, offset);
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

            for other in ctx.all_fragments {
                if other.name.as_ref() == name && other.uri != this.uri {
                    if current_is_public && other.is_public {
                        ctx.diagnostics.push(Diagnostic {
                            range: this.translate_to_file_range(name_node, offset),
                            severity: Some(DiagnosticSeverity::ERROR),
                            message: format!("Duplicate public fragment name: '{}'. Public fragments must have unique names across the workspace.", name),
                            ..Default::default()
                        });
                        break;
                    } else if !current_is_public
                        && !other.is_public
                        && graphox_core::utils::paths_match(
                            this.package_root.as_deref(),
                            other.package_root.as_deref(),
                        )
                    {
                        ctx.diagnostics.push(Diagnostic {
                            range: this.translate_to_file_range(name_node, offset),
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
                            range: this.translate_to_file_range(name_node, offset),
                            severity: Some(DiagnosticSeverity::HINT),
                            message: format!(
                                "Private fragment '{}' shadows a public fragment defined in {}.",
                                name, other.uri
                            ),
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
                        let type_name = this.get_node_text(nt_child, offset);
                        if let Some(type_def) = ctx.schema.types.get(type_name.as_str())
                            && let Some(sel_set) = selection_set_node
                        {
                            crate::diagnostics::selection_set::validate_selection_set(
                                this,
                                sel_set,
                                offset,
                                type_def,
                                ctx,
                                depth + 1,
                                None,
                                None,
                            );
                        }
                    }
                }
            }
        }
    }
}

pub(super) fn validate_inline_fragment(
    this: &DocumentState,
    node: Node,
    offset: usize,
    parent_type: &ExtendedType,
    ctx: &mut ValidationContext,
    depth: usize,
    parent_response_key: Option<&str>,
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
            crate::diagnostics::values::validate_directives(this, child, offset, ctx);
        }
    }

    let mut type_name = None;
    let target_type = if let Some(type_cond) = type_condition_node {
        let mut tc_cursor = type_cond.walk();
        let mut found_type = None;
        for tc_child in type_cond.children(&mut tc_cursor) {
            if tc_child.kind() == "named_type" {
                let mut nt_cursor = tc_child.walk();
                for nt_child in tc_child.children(&mut nt_cursor) {
                    if nt_child.kind() == "name" {
                        type_name = Some(this.get_node_text(nt_child, offset));
                        break;
                    }
                }
            }
        }
        if let Some(name) = &type_name {
            found_type = ctx.schema.types.get(name.as_str());
            if let Some(rk) = parent_response_key {
                ctx.response_key_type_conditions
                    .entry(rk.to_string().into())
                    .or_default()
                    .insert(name.clone().into());
            }
        }
        found_type
    } else {
        Some(parent_type)
    };

    if let Some(t_type) = target_type
        && let Some(sel_set) = selection_set_node
    {
        // Pass parent_response_key through unchanged so fields are tracked under the response key
        // Also pass type_name for tracking in type_condition_fields
        crate::diagnostics::selection_set::validate_selection_set(
            this,
            sel_set,
            offset,
            t_type,
            ctx,
            depth + 1,
            parent_response_key,
            type_name.as_deref(),
        );
    }
}

pub(super) fn validate_fragment_spread(
    this: &DocumentState,
    node: Node,
    offset: usize,
    ctx: &mut ValidationContext,
    parent_response_key: Option<&str>,
    type_name: Option<&str>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "fragment_name" {
            let mut name_cursor = child.walk();
            for name_child in child.children(&mut name_cursor) {
                if name_child.kind() == "name" {
                    let name = this.get_node_text(name_child, offset);
                    let mut visited = ahash::AHashSet::default();
                    let exists = mark_used_variables_recursive(
                        this,
                        &name,
                        ctx,
                        &mut visited,
                        name_child,
                        offset,
                    );

                    if exists && let Some(rk) = parent_response_key {
                        let mut visited_fields = ahash::AHashSet::default();
                        mark_selected_fields_recursive(
                            this,
                            &name,
                            ctx,
                            &mut visited_fields,
                            rk,
                            type_name,
                        );
                    }

                    if !exists && ctx.workspace_loaded {
                        ctx.diagnostics.push(Diagnostic {
                            range: this.translate_to_file_range(name_child, offset),
                            severity: Some(DiagnosticSeverity::ERROR),
                            message: format!("Unknown fragment: {}", name),
                            ..Default::default()
                        });
                    }
                    // If the fragment exists but is marked @type_only, report a warning
                    if exists {
                        // Try to find metadata in workspace fragments first
                        if let Some(meta) =
                            ctx.all_fragments.iter().find(|f| f.name.as_ref() == name)
                        {
                            if meta.is_type_only {
                                // Diagnostic at the spread location; include definition URI + fragment name
                                ctx.diagnostics.push(Diagnostic {
                                    range: this.translate_to_file_range(name_child, offset),
                                    severity: Some(DiagnosticSeverity::WARNING),
                                    message: format!(
                                        "Fragment '{}' is used but marked with @type_only. Remove @type_only to resolve this warning.",
                                        name
                                    ),
                                    code: Some(NumberOrString::String("type_only_used".to_string())),
                                    data: Some(serde_json::json!({
                                        "def_uri": meta.uri.to_string(),
                                        "fragment": name,
                                    })),
                                    ..Default::default()
                                });
                            }
                        } else if let Some(def) =
                            this.fragments().iter().find(|f| f.name.as_ref() == name)
                            && def.is_type_only
                        {
                            // Fragment defined in this document
                            // Try to locate the specific directive node to help code actions
                            let mut directive_range = None;
                            // Need to find the fragment_definition node in this document
                            let query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
                                let lang = tree_sitter_graphql::LANGUAGE.into();
                                tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
                            });
                            let mut qc = tree_sitter::QueryCursor::new();
                            for block in this.get_graphql_trees() {
                                let off = block.offset;
                                let mut matches =
                                    qc.matches(query, block.tree.root_node(), |n: Node| {
                                        let s = n.start_byte();
                                        let e = n.end_byte();
                                        this.rope.byte_slice((s + off)..(e + off)).chunks()
                                    });
                                while let Some(m) = matches.next() {
                                    for cap in m.captures.iter() {
                                        let cap_name = query.capture_names()[cap.index as usize];
                                        if cap_name == "symbol.name" {
                                            let nm = this.get_node_text(cap.node, off);
                                            if nm == name {
                                                // parent container capture will be fragment_definition
                                                // find directives in the fragment container
                                                if let Some(cont) = m.captures.iter().find(|c| {
                                                    query.capture_names()[c.index as usize]
                                                        == "symbol.container"
                                                }) {
                                                    let container = cont.node;
                                                    let mut ccur = container.walk();
                                                    for child in container.children(&mut ccur) {
                                                        if child.kind() == "directives" {
                                                            let mut dcur = child.walk();
                                                            for dir_child in
                                                                child.children(&mut dcur)
                                                            {
                                                                if dir_child.kind() == "directive" {
                                                                    let dir_text = this
                                                                        .get_node_text(
                                                                            dir_child, off,
                                                                        );
                                                                    if dir_text
                                                                        .contains("@type_only")
                                                                    {
                                                                        directive_range = Some(this.translate_to_file_range(dir_child, off));
                                                                        break;
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

                            let mut diag = Diagnostic {
                                range: this.translate_to_file_range(name_child, offset),
                                severity: Some(DiagnosticSeverity::WARNING),
                                message: format!(
                                    "Fragment '{}' is used but marked with @type_only. Remove @type_only to resolve this warning.",
                                    name
                                ),
                                code: Some(NumberOrString::String("type_only_used".to_string())),
                                data: Some(serde_json::json!({
                                    "def_uri": this.uri.to_string(),
                                    "fragment": name,
                                })),
                                ..Default::default()
                            };

                            if let Some(r) = directive_range {
                                // include def_range to allow creating edits without opening doc
                                if let Ok(range_json) = serde_json::to_value(r)
                                    && let Some(ref mut d) = diag.data
                                    && let Some(obj) = d.as_object_mut()
                                {
                                    obj.insert("def_range".to_string(), range_json);
                                }
                            }

                            ctx.diagnostics.push(diag);
                        }
                    }
                }
            }
        } else if child.kind() == "directives" || child.kind() == "directive" {
            crate::diagnostics::values::validate_directives(this, child, offset, ctx);
        }
    }
}

fn mark_used_variables_recursive(
    this: &DocumentState,
    initial_name: &str,
    ctx: &mut ValidationContext,
    visited: &mut ahash::AHashSet<String>,
    trigger_node: Node,
    offset: usize,
) -> bool {
    // Use a DFS traversal to detect cycles and collect used variables.
    fn dfs(
        this: &DocumentState,
        name: &str,
        ctx: &mut ValidationContext,
        visited: &mut ahash::AHashSet<String>,
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
                    if let Some(fmeta) = ctx.all_fragments.iter().find(|f| f.name.as_ref() == *n) {
                        part.push_str(" (");
                        part.push_str(fmeta.uri.path());
                        part.push(')');
                    } else if this.fragments.iter().any(|f| f.name.as_ref() == *n) {
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

            // Only report cycle diagnostics when the trigger node is inside a fragment
            // definition. If the trigger is a fragment spread inside an operation, the
            // diagnostic is redundant (we also report it on the fragment definitions)
            // and causes duplicate reports for the same logical cycle.
            let mut p = trigger_node;
            let mut inside_fragment = false;
            while let Some(parent) = p.parent() {
                if parent.kind() == "fragment_definition" {
                    inside_fragment = true;
                    break;
                }
                p = parent;
            }

            if inside_fragment {
                ctx.diagnostics.push(Diagnostic {
                    range: this.translate_to_file_range(trigger_node, offset),
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: format!("Circular fragment reference: {}", cycle),
                    code: Some(NumberOrString::String("circular_fragment".to_string())),
                    ..Default::default()
                });
            }
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
        if let Some(f) = ctx.all_fragments.iter().find(|f| f.name.as_ref() == name) {
            fragment_exists = true;
            used_variables = Some(&f.used_variables);
            used_fragments = Some(&f.used_fragments);
        } else if let Some(f) = this.fragments.iter().find(|f| f.name.as_ref() == name) {
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
                ctx.used_variables.insert(var.to_string().into());

                if !ctx.defined_variables.is_empty()
                    && !ctx.defined_variables.contains(var.as_ref())
                {
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
        this,
        initial_name,
        ctx,
        local_visited,
        &mut path,
        trigger_node,
        offset,
    )
}

pub(super) fn mark_selected_fields_recursive(
    this: &DocumentState,
    name: &str,
    ctx: &mut ValidationContext,
    visited: &mut ahash::AHashSet<String>,
    response_key: &str,
    type_name: Option<&str>,
) {
    if visited.contains(name) {
        return;
    }
    visited.insert(name.to_string());

    // 1. Try local fragments
    if let Some(frag) = this.fragments().iter().find(|f| f.name.as_ref() == name) {
        for field in &frag.selected_fields {
            if type_name.is_none() {
                ctx.response_key_selected_fields
                    .entry(response_key.to_string().into())
                    .or_default()
                    .insert(field.clone());
            }

            if let Some(tn) = type_name {
                ctx.type_condition_fields
                    .entry(response_key.to_string().into())
                    .or_default()
                    .entry(tn.to_string().into())
                    .or_default()
                    .insert(field.clone());
            }
        }
        for (tc, field) in &frag.type_fields {
            ctx.response_key_type_conditions
                .entry(response_key.to_string().into())
                .or_default()
                .insert(tc.clone());

            ctx.type_condition_fields
                .entry(response_key.to_string().into())
                .or_default()
                .entry(tc.clone())
                .or_default()
                .insert(field.clone());
        }
        for spread in &frag.used_fragments {
            mark_selected_fields_recursive(this, spread, ctx, visited, response_key, type_name);
        }
    }
    // 2. Try workspace fragments
    else if let Some(frag) = ctx.all_fragments.iter().find(|f| f.name.as_ref() == name) {
        for field in &frag.selected_fields {
            if type_name.is_none() {
                ctx.response_key_selected_fields
                    .entry(response_key.to_string().into())
                    .or_default()
                    .insert(field.clone());
            }

            if let Some(tn) = type_name {
                ctx.type_condition_fields
                    .entry(response_key.to_string().into())
                    .or_default()
                    .entry(tn.to_string().into())
                    .or_default()
                    .insert(field.clone());
            }
        }
        for (tc, field) in &frag.type_fields {
            ctx.response_key_type_conditions
                .entry(response_key.to_string().into())
                .or_default()
                .insert(tc.clone());

            ctx.type_condition_fields
                .entry(response_key.to_string().into())
                .or_default()
                .entry(tc.clone())
                .or_default()
                .insert(field.clone());
        }

        // Original loop was here, now redundant but keeping structure for safety
        for spread in &frag.used_fragments {
            mark_selected_fields_recursive(this, spread, ctx, visited, response_key, type_name);
        }
    }
}
