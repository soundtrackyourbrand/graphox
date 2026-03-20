use super::DIAGNOSTIC_SOURCE;
use super::ValidationContext;
use apollo_compiler::ast::OperationType;
use apollo_compiler::schema::ExtendedType;
use graphox_core::document::DocumentState;
use lsp_types::*;
use tree_sitter::Node;

fn push_required_field_diagnostic(diagnostics: &mut Vec<Diagnostic>, diagnostic: Diagnostic) {
    let is_duplicate = diagnostics.iter().any(|existing| {
        existing.range == diagnostic.range
            && existing.message == diagnostic.message
            && matches!(
                (&existing.code, &diagnostic.code),
                (
                    Some(NumberOrString::String(existing_code)),
                    Some(NumberOrString::String(new_code))
                ) if existing_code == new_code
            )
    });

    if !is_duplicate {
        diagnostics.push(diagnostic);
    }
}

fn push_forbidden_field_diagnostic(diagnostics: &mut Vec<Diagnostic>, diagnostic: Diagnostic) {
    let is_duplicate = diagnostics.iter().any(|existing| {
        existing.range == diagnostic.range
            && existing.message == diagnostic.message
            && matches!(
                (&existing.code, &diagnostic.code),
                (
                    Some(NumberOrString::String(existing_code)),
                    Some(NumberOrString::String(new_code))
                ) if existing_code == new_code
            )
    });

    if !is_duplicate {
        diagnostics.push(diagnostic);
    }
}

pub(super) fn validate_operation(
    this: &DocumentState,
    node: Node,
    offset: usize,
    ctx: &mut ValidationContext,
) {
    ctx.is_operation = true;
    ctx.used_variables.clear();
    ctx.defined_variables.clear();
    ctx.response_key_selected_fields.clear();
    ctx.response_key_type_conditions.clear();
    ctx.type_condition_fields.clear();
    ctx.root_response_keys.clear();
    ctx.response_key_anchor_ranges.clear();
    ctx.response_key_types.clear();

    let mut operation_type_string = String::from("query");
    let mut cursor = node.walk();
    let mut var_defs_node = None;

    for child in node.children(&mut cursor) {
        if child.kind() == "operation_type" {
            operation_type_string = this.get_node_text(child, offset);
        } else if child.kind() == "variable_definitions" {
            var_defs_node = Some(child);
        }
    }

    // Set the current operation type
    ctx.current_operation_type = Some(operation_type_string.clone().into());

    // 1. Collect and validate variable definitions
    if let Some(var_defs) = var_defs_node {
        let mut vd_cursor = var_defs.walk();
        for child in var_defs.children(&mut vd_cursor) {
            if child.kind() == "variable_definition" {
                let mut v_cursor = child.walk();
                let mut var_name = None;
                for v_child in child.children(&mut v_cursor) {
                    if v_child.kind() == "variable" {
                        let mut n_cursor = v_child.walk();
                        for n_child in v_child.children(&mut n_cursor) {
                            if n_child.kind() == "name" {
                                var_name = Some(this.get_node_text(n_child, offset));
                            }
                        }
                    } else if v_child.kind() == "type" {
                        validate_type_node(this, v_child, offset, ctx);
                    }
                }

                if let Some(name) = var_name {
                    ctx.defined_variables.insert(name.into());
                }
            }
        }
    }

    // 2. Collect used variables in selection set
    let op_type = match operation_type_string.as_str() {
        "query" => Some(OperationType::Query),
        "mutation" => Some(OperationType::Mutation),
        "subscription" => Some(OperationType::Subscription),
        _ => None,
    };

    if let Some(op) = op_type
        && let Some(root_def_name) = ctx.schema.root_operation(op)
        && let Some(root_type) = ctx.schema.types.get(root_def_name.as_str())
    {
        for child in node.children(&mut cursor) {
            if child.kind() == "selection_set" {
                crate::diagnostics::selection_set::validate_selection_set(
                    this, child, offset, root_type, ctx, 0, None, None,
                );
            }
        }
    }

    // Check required/forbidden fields after validating the selection set
    check_required_fields(this, node, offset, ctx);
    check_forbidden_fields(this, node, offset, ctx);

    // 3. Check for unused variables
    if ctx.workspace_loaded {
        for name in &ctx.defined_variables {
            if !ctx.used_variables.contains(name) {
                // Find the node for this variable definition to report the diagnostic on it
                if let Some(var_defs) = var_defs_node {
                    let mut vd_cursor = var_defs.walk();
                    for child in var_defs.children(&mut vd_cursor) {
                        if child.kind() == "variable_definition" {
                            let mut v_cursor = child.walk();
                            for v_child in child.children(&mut v_cursor) {
                                if v_child.kind() == "variable" {
                                    let mut n_cursor = v_child.walk();
                                    for n_child in v_child.children(&mut n_cursor) {
                                        if n_child.kind() == "name"
                                            && this.get_node_text(n_child, offset) == name.as_ref()
                                        {
                                            ctx.diagnostics.push(Diagnostic {
                                                range: this
                                                    .translate_to_file_range(v_child, offset),
                                                severity: Some(DiagnosticSeverity::WARNING),
                                                message: format!("Unused variable: ${}", name),
                                                code: Some(NumberOrString::String(
                                                    "unused_variable".to_string(),
                                                )),
                                                tags: Some(vec![DiagnosticTag::UNNECESSARY]),
                                                source: DIAGNOSTIC_SOURCE.map(String::from),
                                                ..Default::default()
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
    }
}

pub(super) fn check_required_fields(
    this: &DocumentState,
    node: Node,
    offset: usize,
    ctx: &mut ValidationContext,
) {
    if let Some(config) = ctx.config
        && let Some(operation_type) = ctx.current_operation_type.clone()
    {
        let rules = config.rules();
        let required_fields = rules.required_fields();
        let mut emitted_operation_requirements = ahash::AHashSet::default();
        let mut emitted_response_key_requirements = ahash::AHashSet::default();
        let mut emitted_type_condition_requirements = ahash::AHashSet::default();

        // Find the name node of the operation for the diagnostic range
        let mut cursor = node.walk();
        let name_node = node.children(&mut cursor).find(|c| c.kind() == "name");
        let operation_range = name_node
            .map(|n| this.translate_to_file_range(n, offset))
            .unwrap_or_else(|| this.translate_to_file_range(node, offset));

        for (field_name, rule) in required_fields {
            if !rule.applies_to_operation(operation_type.as_ref()) {
                continue;
            }

            let field_name_str = field_name.as_str();

            // 1. Check root-level required fields (fields on Query/Mutation/Subscription)
            let root_type_name = match operation_type.as_ref() {
                "query" => ctx.schema.root_operation(OperationType::Query),
                "mutation" => ctx.schema.root_operation(OperationType::Mutation),
                "subscription" => ctx.schema.root_operation(OperationType::Subscription),
                _ => None,
            };

            if let Some(rtn) = root_type_name
                && let Some(root_type) = ctx.schema.types.get(rtn.as_str())
            {
                let field_exists_on_root = match root_type {
                    ExtendedType::Object(obj) => obj.fields.contains_key(field_name_str),
                    ExtendedType::Interface(iface) => iface.fields.contains_key(field_name_str),
                    _ => false,
                };

                if field_exists_on_root {
                    // Check if this field was selected at root level
                    let is_selected =
                        ctx.response_key_selected_fields.iter().any(|(rk, fields)| {
                            ctx.root_response_keys.contains(rk) && fields.contains(field_name_str)
                        });

                    if !is_selected {
                        let op_key = format!("{}:{}", field_name_str, operation_type);
                        if !emitted_operation_requirements.insert(op_key) {
                            continue;
                        }

                        let anchor_node =
                            find_root_selection_anchor_for_response_key(this, node, offset, None);
                        if let Some(anchor) = anchor_node
                            && crate::diagnostics::DocumentDiagnostics::has_inline_ignore_comment(
                                this, anchor, offset,
                            )
                        {
                            continue;
                        }

                        push_required_field_diagnostic(
                            ctx.diagnostics,
                            Diagnostic {
                                range: anchor_node
                                    .map(|n| this.translate_to_file_range(n, offset))
                                    .unwrap_or(operation_range),
                                severity: Some(DiagnosticSeverity::ERROR),
                                message: format!(
                                    "Required field '{}' must be selected in {} operations",
                                    field_name, operation_type
                                ),
                                code: Some(NumberOrString::String(
                                    "required_field_missing".to_string(),
                                )),
                                data: Some(serde_json::json!({ "scope": "operation" })),
                                source: DIAGNOSTIC_SOURCE.map(String::from),
                                ..Default::default()
                            },
                        );
                    }
                }
            }

            // 2. Check ALL selected fields (recursive check)
            for (response_key, type_def) in &ctx.response_key_types {
                let empty_set = ahash::AHashSet::default();
                let selected_fields = ctx
                    .response_key_selected_fields
                    .get(response_key)
                    .unwrap_or(&empty_set);

                let field_exists = match type_def {
                    ExtendedType::Object(obj) => obj.fields.contains_key(field_name_str),
                    ExtendedType::Interface(iface) => iface.fields.contains_key(field_name_str),
                    _ => false,
                };

                if field_exists {
                    let mut is_selected = selected_fields.contains(field_name_str);

                    // For object types, fields selected in an inline fragment on the same type also count
                    if !is_selected
                        && let ExtendedType::Object(obj) = type_def
                        && let Some(type_fields) = ctx.type_condition_fields.get(response_key)
                        && let Some(fields) = type_fields.get(obj.name.as_str())
                    {
                        is_selected = fields.contains(field_name_str);
                    }

                    if !is_selected {
                        let response_key_requirement =
                            format!("{}:{}", field_name_str, response_key.as_ref());
                        if !emitted_response_key_requirements.insert(response_key_requirement) {
                            continue;
                        }

                        let Some(anchor_range) = resolve_anchor_and_check_ignore(
                            this,
                            node,
                            offset,
                            ctx,
                            response_key,
                            operation_range,
                        ) else {
                            continue;
                        };

                        push_required_field_diagnostic(
                            ctx.diagnostics,
                            Diagnostic {
                                range: anchor_range,
                                severity: Some(DiagnosticSeverity::ERROR),
                                message: format!(
                                    "Required field '{}' must be selected in '{}'",
                                    field_name, response_key
                                ),
                                code: Some(NumberOrString::String(
                                    "required_field_missing".to_string(),
                                )),
                                data: Some(serde_json::json!({
                                    "scope": "response_key",
                                    "response_key": response_key.as_ref()
                                })),
                                source: DIAGNOSTIC_SOURCE.map(String::from),
                                ..Default::default()
                            },
                        );
                    }
                }
            }

            // 3. Check inline fragment type conditions (merging base selections)
            for (response_key, type_conditions) in &ctx.response_key_type_conditions {
                let base_selected_fields = ctx.response_key_selected_fields.get(response_key);

                for type_name in type_conditions {
                    let type_fields = ctx
                        .type_condition_fields
                        .get(response_key)
                        .and_then(|m| m.get(type_name));

                    let type_name_str = type_name.to_string();
                    if let Some(type_def) = ctx.schema.types.get(&*type_name_str) {
                        let field_exists = match type_def {
                            ExtendedType::Object(obj) => obj.fields.contains_key(field_name_str),
                            ExtendedType::Interface(iface) => {
                                iface.fields.contains_key(field_name_str)
                            }
                            _ => false,
                        };

                        if field_exists {
                            let is_selected = type_fields
                                .is_some_and(|f| f.contains(field_name_str))
                                || base_selected_fields.is_some_and(|f| f.contains(field_name_str));

                            if !is_selected {
                                let type_condition_requirement = format!(
                                    "{}:{}:{}",
                                    field_name_str,
                                    response_key.as_ref(),
                                    type_name
                                );
                                if !emitted_type_condition_requirements
                                    .insert(type_condition_requirement)
                                {
                                    continue;
                                }

                                let Some(anchor_range) = resolve_anchor_and_check_ignore(
                                    this,
                                    node,
                                    offset,
                                    ctx,
                                    response_key,
                                    operation_range,
                                ) else {
                                    continue;
                                };

                                push_required_field_diagnostic(
                                    ctx.diagnostics,
                                    Diagnostic {
                                        range: anchor_range,
                                        severity: Some(DiagnosticSeverity::ERROR),
                                        message: format!(
                                            "Required field '{}' must be selected in '... on {}'",
                                            field_name, type_name
                                        ),
                                        code: Some(NumberOrString::String(
                                            "required_field_missing".to_string(),
                                        )),
                                        data: Some(serde_json::json!({ "scope": "response_key" })),
                                        source: DIAGNOSTIC_SOURCE.map(String::from),
                                        ..Default::default()
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

pub(super) fn check_forbidden_fields(
    this: &DocumentState,
    node: Node,
    offset: usize,
    ctx: &mut ValidationContext,
) {
    if let Some(config) = ctx.config
        && let Some(operation_type) = ctx.current_operation_type.clone()
    {
        let rules = config.rules();
        let forbidden_fields = rules.forbidden_fields();

        for (field_name, rule) in forbidden_fields {
            if !rule.applies_to_operation(operation_type.as_ref()) {
                continue;
            }

            let field_name_str = field_name.as_str();

            // 1. Check root-level forbidden fields (fields on Query/Mutation/Subscription)
            for (response_key, selected_fields) in &ctx.response_key_selected_fields {
                if ctx.root_response_keys.contains(response_key)
                    && selected_fields.contains(field_name_str)
                {
                    // Find the field node for the diagnostic at root level
                    if let Some(field_node) = find_root_field_node_by_name(
                        this,
                        node,
                        offset,
                        response_key,
                        field_name_str,
                    ) {
                        if crate::diagnostics::DocumentDiagnostics::has_inline_ignore_comment(
                            this, field_node, offset,
                        ) {
                            continue;
                        }

                        push_forbidden_field_diagnostic(
                            ctx.diagnostics,
                            Diagnostic {
                                range: this.translate_to_file_range(field_node, offset),
                                severity: Some(DiagnosticSeverity::ERROR),
                                message: format!(
                                    "Field '{}' is forbidden in {} operations",
                                    field_name, operation_type
                                ),
                                code: Some(NumberOrString::String(
                                    "forbidden_field_selected".to_string(),
                                )),
                                data: Some(serde_json::json!({
                                    "scope": "operation",
                                    "field_name": field_name_str
                                })),
                                source: DIAGNOSTIC_SOURCE.map(String::from),
                                ..Default::default()
                            },
                        );
                    }
                }
            }

            // 2. Check all other levels (recursive results)
            for (response_key, selected_fields) in &ctx.response_key_selected_fields {
                if selected_fields.contains(field_name_str) {
                    // Find the field node for the diagnostic
                    if let Some(field_node) =
                        find_field_node_by_name(this, node, offset, response_key, field_name_str)
                    {
                        if crate::diagnostics::DocumentDiagnostics::has_inline_ignore_comment(
                            this, field_node, offset,
                        ) {
                            continue;
                        }

                        push_forbidden_field_diagnostic(
                            ctx.diagnostics,
                            Diagnostic {
                                range: this.translate_to_file_range(field_node, offset),
                                severity: Some(DiagnosticSeverity::ERROR),
                                message: format!(
                                    "Field '{}' is forbidden in {} operations",
                                    field_name, operation_type
                                ),
                                code: Some(NumberOrString::String(
                                    "forbidden_field_selected".to_string(),
                                )),
                                data: Some(serde_json::json!({
                                    "scope": "response_key",
                                    "response_key": response_key.as_ref(),
                                    "field_name": field_name_str
                                })),
                                source: DIAGNOSTIC_SOURCE.map(String::from),
                                ..Default::default()
                            },
                        );
                    }
                }
            }

            // Also check type conditions (inline fragments)
            for (response_key, type_fields) in &ctx.type_condition_fields {
                for (type_name, fields) in type_fields {
                    if fields.contains(field_name_str)
                        && let Some(field_node) = find_field_node_in_type_condition(
                            this,
                            node,
                            offset,
                            response_key,
                            type_name,
                            field_name_str,
                        )
                    {
                        if crate::diagnostics::DocumentDiagnostics::has_inline_ignore_comment(
                            this, field_node, offset,
                        ) {
                            continue;
                        }

                        push_forbidden_field_diagnostic(
                            ctx.diagnostics,
                            Diagnostic {
                                range: this.translate_to_file_range(field_node, offset),
                                severity: Some(DiagnosticSeverity::ERROR),
                                message: format!(
                                    "Field '{}' is forbidden on '... on {}' in {} operations",
                                    field_name, type_name, operation_type
                                ),
                                code: Some(NumberOrString::String(
                                    "forbidden_field_selected".to_string(),
                                )),
                                data: Some(serde_json::json!({
                                    "scope": "response_key",
                                    "response_key": response_key.as_ref(),
                                    "field_name": field_name_str
                                })),
                                source: DIAGNOSTIC_SOURCE.map(String::from),
                                ..Default::default()
                            },
                        );
                    }
                }
            }
        }
    }
}

fn find_root_field_node_by_name<'a>(
    this: &DocumentState,
    node: Node<'a>,
    offset: usize,
    response_key: &str,
    field_name: &str,
) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "selection_set" {
            let mut sel_cursor = child.walk();
            for selection in child.children(&mut sel_cursor) {
                let field_node = if selection.kind() == "selection" {
                    this.find_child_by_kind(selection, "field")
                } else if selection.kind() == "field" {
                    Some(selection)
                } else {
                    None
                };

                if let Some(field) = field_node {
                    let components = this.extract_field_components(field);
                    let mut key = components
                        .name
                        .map(|n| this.get_node_text(n, offset))
                        .unwrap_or_default();
                    if let Some(alias) = components.alias
                        && let Some(alias_name) = this.find_child_by_kind(alias, "name")
                    {
                        key = this.get_node_text(alias_name, offset);
                    }

                    if key == response_key {
                        let name = components
                            .name
                            .map(|n| this.get_node_text(n, offset))
                            .unwrap_or_default();
                        if name == field_name {
                            return Some(field);
                        }
                    }
                }
            }
        }
    }
    None
}

fn find_field_node_by_name<'a>(
    this: &DocumentState,
    node: Node<'a>,
    offset: usize,
    response_key: &str,
    field_name: &str,
) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "selection_set" {
            let mut sel_cursor = child.walk();
            for selection in child.children(&mut sel_cursor) {
                let field_node = if selection.kind() == "selection" {
                    this.find_child_by_kind(selection, "field")
                } else if selection.kind() == "field" {
                    Some(selection)
                } else {
                    None
                };

                if let Some(field) = field_node {
                    let components = this.extract_field_components(field);
                    let mut key = components
                        .name
                        .map(|n| this.get_node_text(n, offset))
                        .unwrap_or_default();
                    if let Some(alias) = components.alias
                        && let Some(alias_name) = this.find_child_by_kind(alias, "name")
                    {
                        key = this.get_node_text(alias_name, offset);
                    }

                    // For find_field_node_by_name (recursive check), the forbidden field
                    // is a selection INSIDE this field.
                    if key == response_key {
                        // Skip if the field itself is the forbidden field (that's root level or other level)
                        // Actually, if key == response_key, we want to look inside.
                        if let Some(selection_set) = components.selection_set {
                            let mut inner_cursor = selection_set.walk();
                            for inner_selection in selection_set.children(&mut inner_cursor) {
                                let inner_field_node = if inner_selection.kind() == "selection" {
                                    this.find_child_by_kind(inner_selection, "field")
                                } else if inner_selection.kind() == "field" {
                                    Some(inner_selection)
                                } else {
                                    None
                                };

                                if let Some(inner_field) = inner_field_node {
                                    let inner_components =
                                        this.extract_field_components(inner_field);
                                    let name = inner_components
                                        .name
                                        .map(|n| this.get_node_text(n, offset))
                                        .unwrap_or_default();
                                    if name == field_name {
                                        return Some(inner_field);
                                    }
                                }
                            }
                        }
                    } else if let Some(_selection_set) = components.selection_set {
                        // Recurse into nested selection sets
                        if let Some(found) = find_field_node_by_name(
                            this,
                            field, // Use field as new root
                            offset,
                            response_key,
                            field_name,
                        ) {
                            return Some(found);
                        }
                    }
                }
            }
        }
    }
    None
}

fn find_field_node_in_type_condition<'a>(
    this: &DocumentState,
    node: Node<'a>,
    offset: usize,
    response_key: &str,
    type_name: &str,
    field_name: &str,
) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "selection_set" {
            let mut sel_cursor = child.walk();
            for selection in child.children(&mut sel_cursor) {
                let field_node = if selection.kind() == "selection" {
                    this.find_child_by_kind(selection, "field")
                } else if selection.kind() == "field" {
                    Some(selection)
                } else {
                    None
                };

                if let Some(field) = field_node {
                    let components = this.extract_field_components(field);
                    let mut key = components
                        .name
                        .map(|n| this.get_node_text(n, offset))
                        .unwrap_or_default();
                    if let Some(alias) = components.alias
                        && let Some(alias_name) = this.find_child_by_kind(alias, "name")
                    {
                        key = this.get_node_text(alias_name, offset);
                    }

                    if key == response_key {
                        if let Some(selection_set) = components.selection_set {
                            let mut inner_cursor = selection_set.walk();
                            for inner_selection in selection_set.children(&mut inner_cursor) {
                                let t = if inner_selection.kind() == "selection" {
                                    inner_selection.child(0)
                                } else {
                                    Some(inner_selection)
                                };

                                if let Some(t) = t
                                    && t.kind() == "inline_fragment"
                                {
                                    let type_cond = this.find_child_by_kind(t, "type_condition");
                                    if let Some(tc) = type_cond
                                        && let Some(name_node) =
                                            this.find_child_by_kind(tc, "named_type")
                                        && this.get_node_text(name_node, offset) == type_name
                                        && let Some(frag_selection_set) =
                                            this.find_child_by_kind(t, "selection_set")
                                    {
                                        let mut frag_cursor = frag_selection_set.walk();
                                        for frag_sel in
                                            frag_selection_set.children(&mut frag_cursor)
                                        {
                                            let frag_field_node = if frag_sel.kind() == "selection"
                                            {
                                                this.find_child_by_kind(frag_sel, "field")
                                            } else if frag_sel.kind() == "field" {
                                                Some(frag_sel)
                                            } else {
                                                None
                                            };

                                            if let Some(frag_field) = frag_field_node {
                                                let frag_components =
                                                    this.extract_field_components(frag_field);
                                                let name = frag_components
                                                    .name
                                                    .map(|n| this.get_node_text(n, offset))
                                                    .unwrap_or_default();
                                                if name == field_name {
                                                    return Some(frag_field);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else if let Some(_selection_set) = components.selection_set {
                        // Recurse into nested selection sets
                        if let Some(found) = find_field_node_in_type_condition(
                            this,
                            field, // Use field as new root
                            offset,
                            response_key,
                            type_name,
                            field_name,
                        ) {
                            return Some(found);
                        }
                    }
                }
            }
        }
    }
    None
}

fn find_root_selection_anchor_for_response_key<'a>(
    this: &DocumentState,
    operation_node: Node<'a>,
    offset: usize,
    response_key: Option<&str>,
) -> Option<Node<'a>> {
    let mut op_cursor = operation_node.walk();
    for child in operation_node.children(&mut op_cursor) {
        if child.kind() != "selection_set" {
            continue;
        }

        let mut sel_cursor = child.walk();
        for selection in child.children(&mut sel_cursor) {
            let field_node = if selection.kind() == "selection" {
                this.find_child_by_kind(selection, "field")
            } else if selection.kind() == "field" {
                Some(selection)
            } else {
                None
            };

            let Some(field) = field_node else {
                continue;
            };

            let components = this.extract_field_components(field);
            let Some(name_node) = components.name else {
                continue;
            };

            let mut key = this.get_node_text(name_node, offset);
            let mut anchor_node = name_node;
            if let Some(alias_node) = components.alias
                && let Some(alias_name_node) = this.find_child_by_kind(alias_node, "name")
            {
                key = this.get_node_text(alias_name_node, offset);
                anchor_node = alias_name_node;
            }

            if response_key.is_none() || response_key == Some(key.as_str()) {
                return Some(anchor_node);
            }
        }
    }

    None
}

fn resolve_anchor_and_check_ignore(
    this: &DocumentState,
    node: Node,
    offset: usize,
    ctx: &ValidationContext,
    response_key: &str,
    operation_range: Range,
) -> Option<Range> {
    if let Some(ranges) = ctx.response_key_anchor_ranges.get(response_key) {
        let mut first_non_ignored = None;
        let mut all_ignored = true;

        for anchor_range in ranges {
            if let Some(anchor_node) = find_node_for_range(this, node, offset, anchor_range)
                && crate::diagnostics::DocumentDiagnostics::has_inline_ignore_comment(
                    this,
                    anchor_node,
                    offset,
                )
            {
                continue;
            }
            all_ignored = false;
            if first_non_ignored.is_none() {
                first_non_ignored = Some(*anchor_range);
            }
        }

        if all_ignored && !ranges.is_empty() {
            return None;
        }

        return Some(first_non_ignored.unwrap_or(operation_range));
    }

    // Fallback to operation level
    if let Some(anchor_node) = find_node_for_range(this, node, offset, &operation_range)
        && crate::diagnostics::DocumentDiagnostics::has_inline_ignore_comment(
            this,
            anchor_node,
            offset,
        )
    {
        return None;
    }

    Some(operation_range)
}

fn find_node_for_range<'a>(
    this: &DocumentState,
    operation_node: Node<'a>,
    offset: usize,
    range: &Range,
) -> Option<Node<'a>> {
    let start_byte = this.position_to_byte(range.start);
    let end_byte = this.position_to_byte(range.end);
    let local_start = start_byte.checked_sub(offset)?;
    let local_end = end_byte.checked_sub(offset)?;
    operation_node.descendant_for_byte_range(local_start, local_end)
}

pub(super) fn validate_type_node(
    this: &DocumentState,
    root: Node,
    offset: usize,
    ctx: &mut ValidationContext,
) {
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "named_type" => {
                    let type_name = this.get_node_text(child, offset);
                    if let Some(type_def) = ctx.schema.types.get(type_name.as_str()) {
                        let directives = type_def.directives();

                        if let Some(directive) = directives.get("deprecated") {
                            let reason = directive
                                .argument_by_name("reason", ctx.schema)
                                .ok()
                                .and_then(|arg| arg.as_str())
                                .unwrap_or("No reason provided");

                            crate::diagnostics::DocumentDiagnostics::add_deprecation_diagnostic(
                                this,
                                ctx,
                                child,
                                offset,
                                format!("Type '{}' is deprecated: {}", type_name, reason),
                                reason,
                            );
                        }
                    }
                }
                "list_type" | "non_null_type" => {
                    stack.push(child);
                }
                _ => {}
            }
        }
    }
}
