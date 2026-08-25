use super::DIAGNOSTIC_SOURCE;
use super::ValidationContext;
use apollo_compiler::schema::ExtendedType;
use graphox_core::document::{DocumentState, PathStep};
use graphox_core::queries::{GQL_SYMBOL_QUERY, GQL_SYMBOL_QUERY_CACHE};
use ls_types::*;
use std::sync::Arc;
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

    // Fragment bodies get the same response-key bookkeeping as operations so the
    // required/forbidden field rules can see nested selections. The maps are
    // per-definition, and one ValidationContext is reused across every
    // definition in the document, so reset them here as well.
    ctx.track_selections = true;
    ctx.response_key_selected_fields.clear();
    ctx.response_key_type_conditions.clear();
    ctx.type_condition_fields.clear();
    ctx.root_response_keys.clear();
    ctx.response_key_anchor_ranges.clear();
    ctx.document_response_keys.clear();
    ctx.fragment_origins.clear();
    ctx.response_key_types.clear();
    ctx.selection_ignores.clear();

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
                source: DIAGNOSTIC_SOURCE.map(String::from),
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
                source: DIAGNOSTIC_SOURCE.map(String::from),
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
                            source: DIAGNOSTIC_SOURCE.map(String::from),
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
                            source: DIAGNOSTIC_SOURCE.map(String::from),
                            ..Default::default()
                        });
                        break;
                    } else if !current_is_public && other.is_public {
                        ctx.diagnostics.push(Diagnostic {
                            range: this.translate_to_file_range(name_node, offset),
                            severity: Some(DiagnosticSeverity::HINT),
                            message: format!(
                                "Private fragment '{}' shadows a public fragment defined in {}.",
                                name,
                                other.uri.as_str()
                            ),
                            source: DIAGNOSTIC_SOURCE.map(String::from),
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

                            let scope = crate::diagnostics::operations::RuleScope::Fragment;
                            crate::diagnostics::operations::check_required_fields(
                                this, node, offset, ctx, scope,
                            );
                            crate::diagnostics::operations::check_forbidden_fields(
                                this, node, offset, ctx, scope,
                            );
                        }
                    }
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn validate_inline_fragment(
    this: &DocumentState,
    node: Node,
    offset: usize,
    parent_type: &ExtendedType,
    ctx: &mut ValidationContext,
    depth: usize,
    parent_response_key: Option<&str>,
    parent_type_condition: Option<&str>,
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
        // No type condition of its own: `... { }` selects on the enclosing
        // type, so it inherits whatever condition is already in effect. Its
        // fields belong wherever a field written in its place would go.
        type_name = parent_type_condition.map(str::to_string);
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

                        mark_nested_selections(
                            this,
                            &name,
                            ctx,
                            rk,
                            this.translate_to_file_range(name_child, offset),
                        );
                    }

                    if !exists && ctx.workspace_loaded {
                        ctx.diagnostics.push(Diagnostic {
                            range: this.translate_to_file_range(name_child, offset),
                            severity: Some(DiagnosticSeverity::ERROR),
                            message: format!("Unknown fragment: {}", name),
                            source: DIAGNOSTIC_SOURCE.map(String::from),
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
                                    source: DIAGNOSTIC_SOURCE.map(String::from),
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
                                source: DIAGNOSTIC_SOURCE.map(String::from),
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
                        part.push_str(&graphox_core::utils::uri_path_text(&fmeta.uri));
                        part.push(')');
                    } else if this.fragments.iter().any(|f| f.name.as_ref() == *n) {
                        part.push_str(" (");
                        part.push_str(&graphox_core::utils::uri_path_text(&this.uri));
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
                    source: DIAGNOSTIC_SOURCE.map(String::from),
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
        let mut used_variables: Option<&[Arc<str>]> = None;
        let mut used_fragments: Option<&[Arc<str>]> = None;
        if let Some(f) = ctx.all_fragments.iter().find(|f| f.name.as_ref() == name) {
            fragment_exists = true;
            used_variables = Some(f.used_variables.as_ref());
            used_fragments = Some(f.used_fragments.as_ref());
        } else if let Some(f) = this.fragments.iter().find(|f| f.name.as_ref() == name) {
            fragment_exists = true;
            used_variables = Some(f.used_variables.as_ref());
            used_fragments = Some(f.used_fragments.as_ref());
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
                        source: DIAGNOSTIC_SOURCE.map(String::from),
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

/// The type condition a fragment definition carries, wherever it is defined.
/// Local definitions win over workspace ones, matching how their fields are
/// collected everywhere else.
pub(super) fn fragment_type_condition(
    this: &DocumentState,
    all_fragments: &[crate::completion::FragmentCompletionInfo],
    name: &str,
) -> Option<Arc<str>> {
    this.fragments()
        .iter()
        .find(|f| f.name.as_ref() == name)
        .map(|f| f.type_condition.clone())
        .or_else(|| {
            all_fragments
                .iter()
                .find(|f| f.name.as_ref() == name)
                .map(|f| f.type_condition.clone())
        })
}

/// A fragment spread whose own type condition is a subtype of what is in
/// effect narrows it exactly as `... on X` would. The abstract type has no
/// fields of its own for a field rule to be about, so the condition has to
/// travel with the fields or the selection is never checked against anything.
///
/// `in_effect` is the type condition already in force, or the response key's own
/// type when there is none. A spread narrows relative to that rather than to the
/// key: a fragment on an interface spread at a union key narrows to the
/// interface, and a fragment on a member spread inside *that* narrows again to
/// the member. Stopping after the first step files the member's fields under the
/// interface, where no rule about the member ever looks for them.
///
/// Direction matters. A fragment written on a *supertype* — `on Node` spread
/// where a `Pet` is in effect — selects for every possible type of what is in
/// effect, so its fields belong to the key itself, not to a narrower condition.
/// Recording them under the supertype hides them from every rule about the
/// type actually being selected, and the field looks unselected when it is not.
///
/// None when the spread narrows nothing: what is in effect is a concrete type,
/// the fragment is written on that very type, or its condition is not a subtype
/// of it.
fn narrowing_type_condition(
    schema: &apollo_compiler::validation::Valid<apollo_compiler::Schema>,
    in_effect: &str,
    fragment_type_condition: &str,
) -> Option<Arc<str>> {
    if in_effect == fragment_type_condition {
        return None;
    }
    // Covers an object implementing an interface, an interface refining
    // another, and a member of a union — and rules out the reverse of each.
    if !schema.is_subtype(in_effect, fragment_type_condition) {
        return None;
    }
    Some(Arc::from(fragment_type_condition))
}

/// The per-selection ignore list a fragment carries, from wherever it is
/// defined. Empty for almost every fragment, so callers can skip cheaply.
fn selection_ignores_of(
    this: &DocumentState,
    all_fragments: &[crate::completion::FragmentCompletionInfo],
    name: &str,
) -> Vec<graphox_core::document::SelectionIgnore> {
    this.fragments()
        .iter()
        .find(|f| f.name.as_ref() == name)
        .map(|f| f.selection_ignores.to_vec())
        .or_else(|| {
            all_fragments
                .iter()
                .find(|f| f.name.as_ref() == name)
                .map(|f| f.selection_ignores.to_vec())
        })
        .unwrap_or_default()
}

/// Key for the guard against walking one fragment twice. A fragment reached
/// under two different type conditions contributes to both, so the condition is
/// part of its identity here. The separator cannot occur in a GraphQL name.
fn visit_key(name: &str, type_name: Option<&str>) -> String {
    match type_name {
        Some(tn) => format!("{name}\u{1f}{tn}"),
        None => name.to_string(),
    }
}

pub(super) fn mark_selected_fields_recursive(
    this: &DocumentState,
    name: &str,
    ctx: &mut ValidationContext,
    visited: &mut ahash::AHashSet<String>,
    response_key: &str,
    type_name: Option<&str>,
) {
    // Whatever is in effect here — an enclosing `... on X`, or the key's own
    // type — this spread's own condition may narrow it further.
    let in_effect: Option<String> = type_name.map(str::to_string).or_else(|| {
        ctx.response_key_types
            .get(response_key)
            .map(|t| t.name().to_string())
    });
    let narrowed = in_effect.as_deref().and_then(|in_effect| {
        fragment_type_condition(this, ctx.all_fragments, name)
            .and_then(|tc| narrowing_type_condition(ctx.schema, in_effect, &tc))
    });
    // The narrower condition wins: it is the one the fields belong to.
    let type_name = narrowed.as_deref().or(type_name);

    // One fragment can be reached under two different conditions — two members
    // of a union both spreading the same `on Node` fragment, say. Its fields
    // belong under each, so the guard has to key on the pair or the second
    // arrival is dropped and that member looks unsatisfied.
    if !visited.insert(visit_key(name, type_name)) {
        return;
    }

    // Suppression written on a selection at this fragment's own top level
    // belongs to the key it is spread under, which is where the rule about that
    // field is evaluated.
    for (path, field, scope) in selection_ignores_of(this, ctx.all_fragments, name) {
        if path.is_empty() {
            ctx.selection_ignores
                .insert((response_key.to_string().into(), field), scope);
        }
    }

    // The required check discovers which conditions exist at a key from this
    // set, so a narrowing spread has to register itself the way `... on X`
    // does or its fields are recorded and never looked at.
    if let Some(tc) = &narrowed {
        ctx.response_key_type_conditions
            .entry(response_key.to_string().into())
            .or_default()
            .insert(tc.clone());
    }

    // 1. Try local fragments
    if let Some(frag) = this.fragments().iter().find(|f| f.name.as_ref() == name) {
        for field in frag.selected_fields.iter() {
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
        for (tc, field) in frag.type_fields.iter() {
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
        for spread in frag.top_level_spreads.iter() {
            mark_selected_fields_recursive(this, spread, ctx, visited, response_key, type_name);
        }
    }
    // 2. Try workspace fragments
    else if let Some(frag) = ctx.all_fragments.iter().find(|f| f.name.as_ref() == name) {
        for field in frag.selected_fields.iter() {
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
        for (tc, field) in frag.type_fields.iter() {
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

        for spread in frag.top_level_spreads.iter() {
            mark_selected_fields_recursive(this, spread, ctx, visited, response_key, type_name);
        }
    }
}

/// Whether spreading `name` contributes `field_name` under the type condition
/// `target`, where `enclosing` is the condition already in effect at the spread
/// and `key_type` is the type of the response key it sits under.
///
/// This mirrors `mark_selected_fields_recursive`, which is what recorded the
/// field: the two have to agree, or a field is tracked under a condition and
/// then has no node the diagnostic can point at.
#[allow(clippy::too_many_arguments)]
pub(super) fn spread_contributes_field_under(
    this: &DocumentState,
    schema: &apollo_compiler::validation::Valid<apollo_compiler::Schema>,
    all_fragments: &[crate::completion::FragmentCompletionInfo],
    name: &str,
    field_name: &str,
    target: Option<&str>,
    enclosing: Option<&str>,
    key_type: Option<&ExtendedType>,
    visited: &mut ahash::AHashSet<String>,
    depth: usize,
) -> bool {
    // The visited guard bounds this by the number of distinct fragments, which
    // on a large workspace is deeper than a worker thread's stack. Same limit
    // and same reasoning as the nested-selection walk below.
    if depth > MAX_NESTED_SPREAD_DEPTH {
        return false;
    }

    let in_effect: Option<String> = enclosing
        .map(str::to_string)
        .or_else(|| key_type.map(|t| t.name().to_string()));
    let narrowed = in_effect.as_deref().and_then(|in_effect| {
        fragment_type_condition(this, all_fragments, name)
            .and_then(|tc| narrowing_type_condition(schema, in_effect, &tc))
    });
    let effective: Option<&str> = narrowed.as_deref().or(enclosing);

    // Keyed on the pair for the same reason as the recorder: one fragment can
    // legitimately be reached under two conditions.
    if !visited.insert(visit_key(name, effective)) {
        return false;
    }

    let local = this
        .fragments()
        .iter()
        .find(|f| f.name.as_ref() == name)
        .map(|f| {
            (
                &f.selected_fields,
                &f.type_fields,
                &f.top_level_spreads,
                &f.nested_selections,
            )
        });
    let Some((selected_fields, type_fields, top_level_spreads, nested_selections)) =
        local.or_else(|| {
            all_fragments
                .iter()
                .find(|f| f.name.as_ref() == name)
                .map(|f| {
                    (
                        &f.selected_fields,
                        &f.type_fields,
                        &f.top_level_spreads,
                        &f.nested_selections,
                    )
                })
        })
    else {
        return false;
    };

    // Top-level fields land wherever the condition in effect puts them.
    if effective == target && selected_fields.iter().any(|f| f.as_ref() == field_name) {
        return true;
    }

    // An `... on X` block inside the fragment records under X regardless.
    if let Some(t) = target
        && type_fields
            .iter()
            .any(|(tc, f)| tc.as_ref() == t && f.as_ref() == field_name)
    {
        return true;
    }

    if top_level_spreads.iter().any(|spread| {
        spread_contributes_field_under(
            this,
            schema,
            all_fragments,
            spread,
            field_name,
            target,
            effective,
            key_type,
            visited,
            depth + 1,
        )
    }) {
        return true;
    }

    // A spread inside `... on X` at the fragment's own top level opens no
    // response key, so it is not a top-level spread and has no path either. It
    // sits in the nested metadata with an empty path, and what it contributes
    // belongs to X.
    nested_selections
        .iter()
        .filter(|entry| entry.is_spread && entry.path.is_empty())
        .any(|entry| {
            spread_contributes_field_under(
                this,
                schema,
                all_fragments,
                &entry.name,
                field_name,
                target,
                entry.type_condition.as_deref().or(effective),
                key_type,
                visited,
                depth + 1,
            )
        })
}

/// Record the objects nested inside a spread fragment as selections of the
/// document that spreads it.
///
/// A fragment's top-level fields merge into the response key it is spread under,
/// which `mark_selected_fields_recursive` handles. Anything below that has no
/// response key here at all, so each nested path gets a synthetic one carrying
/// the type resolved from the schema and an anchor on the spread. Both field
/// rules are driven by that bookkeeping, so recording it is all they need in
/// order to see nested selections.
pub(super) fn mark_nested_selections(
    this: &DocumentState,
    name: &str,
    ctx: &mut ValidationContext,
    base_key: &str,
    spread_range: Range,
) {
    let mut walk = NestedWalk {
        chain: ahash::AHashSet::default(),
        budget: 4096,
        spread_parent: Arc::from(base_key),
    };
    mark_nested_selections_inner(this, name, ctx, base_key, spread_range, &mut walk, 0);
}

/// Bookkeeping for one nested-selection walk. Fragment cycles are invalid
/// GraphQL but reach us while a document is being edited, and the paths this
/// walk builds grow as it descends, so a visited set keyed on the path would
/// never see a repeat and the walk would not terminate. The chain below is
/// keyed on the fragment instead.
struct NestedWalk {
    /// Fragments on the current spread chain. A fragment can be expanded at
    /// several paths, but never inside its own expansion.
    chain: ahash::AHashSet<Arc<str>>,
    /// Remaining expansions, so a fragment graph that fans out sharply cannot
    /// make the walk explode. Each one is a metadata lookup and a few map
    /// inserts, so this is a backstop rather than a working limit.
    budget: usize,
    /// The document's own response key that holds the spread this walk started
    /// from. Constant for the walk, however deep it goes.
    spread_parent: Arc<str>,
}

/// Spread nesting this walk follows. The chain guard alone bounds recursion by
/// the number of distinct fragments, which on a large workspace is deeper than a
/// worker thread's stack; nothing legitimate nests anywhere near this far.
const MAX_NESTED_SPREAD_DEPTH: usize = 64;

fn mark_nested_selections_inner(
    this: &DocumentState,
    name: &str,
    ctx: &mut ValidationContext,
    base_key: &str,
    spread_range: Range,
    walk: &mut NestedWalk,
    depth: usize,
) {
    if walk.budget == 0 || depth > MAX_NESTED_SPREAD_DEPTH {
        return;
    }
    walk.budget -= 1;

    let chain_key: Arc<str> = Arc::from(name);
    if !walk.chain.insert(chain_key.clone()) {
        return;
    }

    // Arc clones, so the fragment metadata is not borrowed while ctx is written.
    let all_fragments = ctx.all_fragments;
    let local = this
        .fragments()
        .iter()
        .find(|f| f.name.as_ref() == name)
        .map(|f| {
            (
                f.type_condition.clone(),
                f.nested_selections.clone(),
                f.top_level_spreads.clone(),
            )
        });
    let Some((type_condition, nested_selections, top_level_spreads)) = local.or_else(|| {
        all_fragments
            .iter()
            .find(|f| f.name.as_ref() == name)
            .map(|f| {
                (
                    f.type_condition.clone(),
                    f.nested_selections.clone(),
                    f.top_level_spreads.clone(),
                )
            })
    }) else {
        walk.chain.remove(&chain_key);
        return;
    };

    // Suppression on the nested selections themselves, at the keys they land on.
    let own_ignores = selection_ignores_of(this, all_fragments, name);
    if !own_ignores.is_empty() {
        for (path, field, scope) in &own_ignores {
            if path.is_empty() {
                continue;
            }
            let key: Arc<str> = format!("{}.{}", base_key, path).into_boxed_str().into();
            ctx.selection_ignores.insert((key, field.clone()), *scope);
        }
    }

    for entry in nested_selections.iter() {
        let Some(type_def) =
            resolve_path_type(ctx.schema, type_condition.as_ref(), &entry.type_path)
        else {
            continue;
        };

        // The path the fragment nests is a response key of the consuming
        // document like any other, so selections merge with the same path
        // selected inline: `zone { account { id } ...F }` is one object.
        //
        // An empty path means the selection sits at the fragment's own top
        // level, under a type condition rather than below a field — a spread
        // inside `... on X` opens no key of its own. It belongs to the key the
        // fragment is spread under, so joining a path onto it would invent a
        // key (`item.`) that nothing else ever names, and the selection would
        // be recorded somewhere no rule looks.
        let nested_key: Arc<str> = if entry.path.is_empty() {
            Arc::from(base_key)
        } else {
            format!("{}.{}", base_key, entry.path)
                .into_boxed_str()
                .into()
        };

        ctx.response_key_types
            .entry(nested_key.clone())
            .or_insert(type_def);
        ctx.fragment_origins
            .entry(nested_key.clone())
            .or_insert_with(|| crate::diagnostics::FragmentOrigin {
                fragment: Arc::from(name),
                anchor: spread_range,
                spread_parent: walk.spread_parent.clone(),
                ignored: entry.path_ignored,
            });

        if entry.is_spread {
            let mut visited_fields = ahash::AHashSet::default();
            mark_selected_fields_recursive(
                this,
                &entry.name,
                ctx,
                &mut visited_fields,
                &nested_key,
                entry.type_condition.as_deref(),
            );
            mark_nested_selections_inner(
                this,
                &entry.name,
                ctx,
                &nested_key,
                spread_range,
                walk,
                depth + 1,
            );
        } else if let Some(tc) = &entry.type_condition {
            ctx.response_key_type_conditions
                .entry(nested_key.clone())
                .or_default()
                .insert(tc.clone());
            ctx.type_condition_fields
                .entry(nested_key)
                .or_default()
                .entry(tc.clone())
                .or_default()
                .insert(entry.name.clone());
        } else {
            ctx.response_key_selected_fields
                .entry(nested_key)
                .or_default()
                .insert(entry.name.clone());
        }
    }

    // A top-level spread's own nested objects sit at the same base key.
    for spread in top_level_spreads.iter() {
        mark_nested_selections_inner(this, spread, ctx, base_key, spread_range, walk, depth + 1);
    }

    walk.chain.remove(&chain_key);
}

/// Walk `type_path` from `start_type` and return the type it lands on. None if
/// the schema does not have that path, which happens while a document is
/// mid-edit.
fn resolve_path_type(
    schema: &apollo_compiler::validation::Valid<apollo_compiler::Schema>,
    start_type: &str,
    type_path: &[PathStep],
) -> Option<ExtendedType> {
    let mut current = schema.types.get(start_type)?.clone();
    for step in type_path {
        current = match step {
            PathStep::TypeCondition(name) => schema.types.get(&**name)?.clone(),
            PathStep::Field(name) => {
                let field_def = match &current {
                    ExtendedType::Object(obj) => obj.fields.get(&**name)?,
                    ExtendedType::Interface(iface) => iface.fields.get(&**name)?,
                    _ => return None,
                };
                schema
                    .types
                    .get(field_def.ty.inner_named_type().as_str())?
                    .clone()
            }
        };
    }
    Some(current)
}
