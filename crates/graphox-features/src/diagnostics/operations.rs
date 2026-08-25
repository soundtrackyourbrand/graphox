use super::DIAGNOSTIC_SOURCE;
use super::ValidationContext;
use apollo_compiler::ast::OperationType;
use apollo_compiler::schema::ExtendedType;
use graphox_core::document::DocumentState;
use graphox_core::document::IgnoreRule;
use ls_types::*;
use tree_sitter::Node;

/// Which kind of definition a required/forbidden field check is running inside.
#[derive(Clone, Copy)]
pub(super) enum RuleScope<'a> {
    /// An operation definition. Every rule can be evaluated, including the
    /// root-level requirements on Query/Mutation/Subscription.
    Operation(&'a str),
    /// A fragment definition. The enclosing operation is unknown, so
    /// operation-scoped rules are skipped. The fragment's own type condition is
    /// also left alone: the consuming operation merges the fragment's top-level
    /// fields into the response key it is spread under, so only the nested
    /// selections inside the fragment body need checking here.
    Fragment,
}

impl<'a> RuleScope<'a> {
    fn operation_type(&self) -> Option<&'a str> {
        match self {
            RuleScope::Operation(op) => Some(op),
            RuleScope::Fragment => None,
        }
    }

    /// Whether a rule can be evaluated in this scope.
    fn allows(&self, rule: &graphox_core::config::FieldRule) -> bool {
        match self {
            RuleScope::Operation(op) => rule.applies_to_operation(op),
            RuleScope::Fragment => rule.applies_to_any_operation(),
        }
    }
}

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
    ctx.track_selections = true;
    ctx.used_variables.clear();
    ctx.defined_variables.clear();
    ctx.response_key_selected_fields.clear();
    ctx.response_key_type_conditions.clear();
    ctx.type_condition_fields.clear();
    ctx.root_response_keys.clear();
    ctx.response_key_anchor_ranges.clear();
    ctx.document_response_keys.clear();
    ctx.fragment_origins.clear();
    ctx.response_key_types.clear();
    ctx.selection_ignores.clear();

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
    let scope = RuleScope::Operation(operation_type_string.as_str());
    check_required_fields(this, node, offset, ctx, scope);
    check_forbidden_fields(this, node, offset, ctx, scope);

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
    scope: RuleScope,
) {
    if let Some(config) = ctx.config {
        let rules = config.rules();
        let mut emitted_operation_requirements = ahash::AHashSet::default();
        let mut emitted_response_key_requirements = ahash::AHashSet::default();
        let mut emitted_type_condition_requirements = ahash::AHashSet::default();

        // Range of the enclosing definition, used as a fallback anchor
        let definition_range = definition_name_range(this, node, offset);

        // 1. Check root-level required fields (fields on Query/Mutation/Subscription).
        // Only meaningful inside an operation; see `RuleScope::Fragment`.
        let root_type_name = match scope.operation_type() {
            Some("query") => ctx.schema.root_operation(OperationType::Query),
            Some("mutation") => ctx.schema.root_operation(OperationType::Mutation),
            Some("subscription") => ctx.schema.root_operation(OperationType::Subscription),
            _ => None,
        };

        if let Some(operation_type) = scope.operation_type()
            && let Some(rtn) = root_type_name
            && let Some(root_type) = ctx.schema.types.get(rtn.as_str())
        {
            let rtn_str = rtn.as_str();
            let fields_to_check = match root_type {
                ExtendedType::Object(obj) => obj.fields.keys().collect::<Vec<_>>(),
                ExtendedType::Interface(iface) => iface.fields.keys().collect::<Vec<_>>(),
                _ => vec![],
            };

            for field_name_str in fields_to_check {
                if let Some(rule) = rules.get_required_rule(rtn_str, field_name_str) {
                    if !rule.applies_to_operation(operation_type) {
                        continue;
                    }

                    // Check if this field was selected at root level
                    let is_selected =
                        ctx.response_key_selected_fields.iter().any(|(rk, fields)| {
                            ctx.root_response_keys.contains(rk)
                                && fields.contains(field_name_str.as_str())
                        });

                    if !is_selected {
                        let op_key = format!("{}:{}:{}", rtn_str, field_name_str, operation_type);
                        if !emitted_operation_requirements.insert(op_key) {
                            continue;
                        }

                        let anchor_node =
                            find_root_selection_anchor_for_response_key(this, node, offset, None);
                        if let Some(anchor) = anchor_node
                            && this.ignore_covers(anchor, offset, IgnoreRule::RequiredFields)
                        {
                            continue;
                        }

                        push_required_field_diagnostic(
                            ctx.diagnostics,
                            Diagnostic {
                                range: anchor_node
                                    .map(|n| this.translate_to_file_range(n, offset))
                                    .unwrap_or(definition_range),
                                severity: Some(DiagnosticSeverity::ERROR),
                                message: format!(
                                    "Required field '{}' must be selected in {} operations{}",
                                    field_name_str,
                                    operation_type,
                                    rule.reason()
                                        .map(|r| format!(": {}", r))
                                        .unwrap_or_default()
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
        }

        // 2. Check ALL selected fields (recursive check)
        for (response_key, type_def) in &ctx.response_key_types {
            let type_name = type_def.name().as_str();
            // `response_key` is the full path from the operation root (e.g.
            // "account.billing.subscription"); use the leaf for user-facing text.
            let display_key = response_key
                .as_ref()
                .rsplit('.')
                .next()
                .unwrap_or(response_key.as_ref());

            // We need to check ALL fields defined on this type in the schema,
            // because a required field might be missing entirely from the selection.

            let fields_to_check = match type_def {
                ExtendedType::Object(obj) => obj.fields.keys().collect::<Vec<_>>(),
                ExtendedType::Interface(iface) => iface.fields.keys().collect::<Vec<_>>(),
                _ => vec![],
            };

            for field_name_str in fields_to_check {
                if let Some(rule) = rules.get_required_rule(type_name, field_name_str) {
                    if !scope.allows(rule) {
                        continue;
                    }

                    // A key that only a fragment declares carries just the
                    // rules the fragment's own pass cannot evaluate; see
                    // check_forbidden_fields.
                    let origin = fragment_only_origin(ctx, response_key).cloned();
                    if origin.is_some() && rule.applies_to_any_operation() {
                        continue;
                    }

                    let empty_set = ahash::AHashSet::default();
                    let selected_fields = ctx
                        .response_key_selected_fields
                        .get(response_key)
                        .unwrap_or(&empty_set);

                    let mut is_selected = selected_fields.contains(field_name_str.as_str());

                    // For object types, fields selected in an inline fragment on the same type also count
                    if !is_selected
                        && let ExtendedType::Object(obj) = type_def
                        && let Some(type_fields) = ctx.type_condition_fields.get(response_key)
                        && let Some(fields) = type_fields.get(obj.name.as_str())
                    {
                        is_selected = fields.contains(field_name_str.as_str());
                    }

                    if !is_selected {
                        let response_key_requirement =
                            format!("{}:{}:{}", type_name, field_name_str, response_key.as_ref());
                        if !emitted_response_key_requirements.insert(response_key_requirement) {
                            continue;
                        }

                        let anchor_range = match &origin {
                            Some(origin) => {
                                if nested_selection_ignored(
                                    this,
                                    node,
                                    offset,
                                    ctx,
                                    origin,
                                    definition_range,
                                    IgnoreRule::RequiredFields,
                                ) {
                                    continue;
                                }
                                origin.anchor
                            }
                            None => {
                                let Some(anchor_range) = resolve_anchor_and_check_ignore(
                                    this,
                                    node,
                                    offset,
                                    ctx,
                                    response_key,
                                    definition_range,
                                    Some(IgnoreRule::RequiredFields),
                                ) else {
                                    continue;
                                };
                                anchor_range
                            }
                        };

                        push_required_field_diagnostic(
                            ctx.diagnostics,
                            Diagnostic {
                                range: anchor_range,
                                severity: Some(DiagnosticSeverity::ERROR),
                                message: format!(
                                    "Required field '{}' must be selected in '{}'{}{}",
                                    field_name_str,
                                    display_key,
                                    nested_in_fragment_suffix(
                                        origin.as_ref().map(|o| o.fragment.as_ref())
                                    ),
                                    rule.reason()
                                        .map(|r| format!(": {}", r))
                                        .unwrap_or_default()
                                ),
                                code: Some(NumberOrString::String(
                                    "required_field_missing".to_string(),
                                )),
                                data: Some(serde_json::json!({
                                    "scope": "response_key",
                                    "response_key": display_key
                                })),
                                source: DIAGNOSTIC_SOURCE.map(String::from),
                                ..Default::default()
                            },
                        );
                    }
                }
            }
        }

        // 3. Check inline fragment type conditions (merging base selections)
        for (response_key, type_conditions) in &ctx.response_key_type_conditions {
            let base_selected_fields = ctx.response_key_selected_fields.get(response_key);

            for type_name in type_conditions {
                let type_name_str = type_name.to_string();
                if let Some(type_def) = ctx.schema.types.get(&*type_name_str) {
                    let fields_to_check = match type_def {
                        ExtendedType::Object(obj) => obj.fields.keys().collect::<Vec<_>>(),
                        ExtendedType::Interface(iface) => iface.fields.keys().collect::<Vec<_>>(),
                        _ => vec![],
                    };

                    for field_name_str in fields_to_check {
                        if let Some(rule) = rules.get_required_rule(&type_name_str, field_name_str)
                        {
                            if !scope.allows(rule) {
                                continue;
                            }

                            let origin = fragment_only_origin(ctx, response_key).cloned();
                            if origin.is_some() && rule.applies_to_any_operation() {
                                continue;
                            }

                            let type_fields = ctx
                                .type_condition_fields
                                .get(response_key)
                                .and_then(|m| m.get(type_name));

                            let is_selected = type_fields
                                .is_some_and(|f| f.contains(field_name_str.as_str()))
                                || base_selected_fields
                                    .is_some_and(|f| f.contains(field_name_str.as_str()));

                            if !is_selected {
                                let type_condition_requirement = format!(
                                    "{}:{}:{}:{}",
                                    type_name_str,
                                    field_name_str,
                                    response_key.as_ref(),
                                    type_name
                                );
                                if !emitted_type_condition_requirements
                                    .insert(type_condition_requirement)
                                {
                                    continue;
                                }

                                let anchor_range = match &origin {
                                    Some(origin) => {
                                        if nested_selection_ignored(
                                            this,
                                            node,
                                            offset,
                                            ctx,
                                            origin,
                                            definition_range,
                                            IgnoreRule::RequiredFields,
                                        ) {
                                            continue;
                                        }
                                        origin.anchor
                                    }
                                    None => {
                                        let Some(anchor_range) = resolve_anchor_and_check_ignore(
                                            this,
                                            node,
                                            offset,
                                            ctx,
                                            response_key,
                                            definition_range,
                                            Some(IgnoreRule::RequiredFields),
                                        ) else {
                                            continue;
                                        };
                                        anchor_range
                                    }
                                };

                                push_required_field_diagnostic(
                                    ctx.diagnostics,
                                    Diagnostic {
                                        range: anchor_range,
                                        severity: Some(DiagnosticSeverity::ERROR),
                                        message: format!(
                                            "Required field '{}' must be selected in '... on {}'{}{}",
                                            field_name_str,
                                            type_name,
                                            nested_in_fragment_suffix(
                                                origin.as_ref().map(|o| o.fragment.as_ref())
                                            ),
                                            rule.reason()
                                                .map(|r| format!(": {}", r))
                                                .unwrap_or_default()
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
    scope: RuleScope,
) {
    if let Some(config) = ctx.config {
        let rules = config.rules();

        // Range of the enclosing definition, used as a fallback anchor
        let definition_range = definition_name_range(this, node, offset);

        // 1. Check all selected fields (recursive check)
        for (response_key, selected_fields) in &ctx.response_key_selected_fields {
            if let Some(type_def) = ctx.response_key_types.get(response_key) {
                let type_name = type_def.name().as_str();

                for field_name_str in selected_fields {
                    let rule = rules.get_forbidden_rule(type_name, field_name_str);
                    if let Some(rule) = rule {
                        if !scope.allows(rule) {
                            continue;
                        }

                        // Find the field node for the diagnostic
                        // The forbidden field is a selection INSIDE this response_key
                        // OR it IS the root selection itself (if the root type has a forbidden field)

                        let field_node = if ctx.root_response_keys.contains(response_key) {
                            // Check if the root selection itself is the forbidden field
                            // (e.g. Query.users is forbidden)
                            let root_type_name = match scope.operation_type() {
                                Some("query") => ctx.schema.root_operation(OperationType::Query),
                                Some("mutation") => {
                                    ctx.schema.root_operation(OperationType::Mutation)
                                }
                                Some("subscription") => {
                                    ctx.schema.root_operation(OperationType::Subscription)
                                }
                                _ => None,
                            };

                            let mut found_node = None;
                            if let Some(rtn) = root_type_name
                                && rtn.as_str() == type_name
                                && let Some(n) = find_root_field_node_by_name(
                                    this,
                                    node,
                                    offset,
                                    response_key,
                                    field_name_str,
                                )
                            {
                                found_node = Some(n);
                            }

                            if found_node.is_none() {
                                // It's a field inside the root selection
                                find_field_node_by_name(
                                    this,
                                    node,
                                    offset,
                                    response_key,
                                    field_name_str,
                                )
                            } else {
                                found_node
                            }
                        } else {
                            find_field_node_by_name(
                                this,
                                node,
                                offset,
                                response_key,
                                field_name_str,
                            )
                        };

                        // A key only a fragment declares stands for an object
                        // nested in its body: nothing here to point at, so the
                        // spread carries the diagnostic. The fragment's own pass
                        // already covers rules that hold in every operation, so
                        // only operation-scoped ones are evaluated here.
                        let origin = fragment_only_origin(ctx, response_key).cloned();
                        if origin.is_some() && rule.applies_to_any_operation() {
                            continue;
                        }

                        let mut via_fragment =
                            origin.as_ref().map(|o| o.fragment.as_ref().to_string());

                        // Otherwise the field may still have been merged in
                        // from a spread at this response key.
                        let mut field_node = field_node;
                        if origin.is_none()
                            && field_node.is_none()
                            && let Some((spread_node, fragment_name)) =
                                find_fragment_spread_selecting_field(
                                    this,
                                    node,
                                    offset,
                                    ctx.all_fragments,
                                    response_key,
                                    field_name_str,
                                    None,
                                    ctx.response_key_types.get(response_key),
                                    ctx.schema,
                                )
                        {
                            field_node = Some(spread_node);
                            via_fragment = Some(fragment_name);
                        }

                        if origin.is_some() || field_node.is_some() {
                            let anchor_range = match &origin {
                                Some(origin) => {
                                    if nested_selection_ignored(
                                        this,
                                        node,
                                        offset,
                                        ctx,
                                        origin,
                                        definition_range,
                                        IgnoreRule::ForbiddenFields,
                                    ) {
                                        continue;
                                    }
                                    origin.anchor
                                }
                                None => {
                                    let Some(anchor_range) = resolve_anchor_and_check_ignore(
                                        this,
                                        node,
                                        offset,
                                        ctx,
                                        response_key,
                                        definition_range,
                                        None,
                                    ) else {
                                        continue;
                                    };
                                    anchor_range
                                }
                            };

                            // A forbidden field is present, so it is silenced
                            // on itself — the narrowest thing there is, and the
                            // line the diagnostic points at. That holds whether
                            // the field is written here or in the body of a
                            // fragment spread here, which is why the answer
                            // comes from the collected scopes rather than from
                            // a node in this document.
                            if selection_ignored(
                                ctx,
                                response_key,
                                field_name_str,
                                IgnoreRule::ForbiddenFields,
                            ) {
                                continue;
                            }

                            // `field_node` is the field itself when it is
                            // written here, and the spread that merged it in
                            // when it is not. Either is a placement that
                            // covers this finding: the field is the narrowest
                            // thing there is, and a spread is a leaf.
                            if let Some(field_node) = field_node
                                && this.ignore_covers(
                                    field_node,
                                    offset,
                                    IgnoreRule::ForbiddenFields,
                                )
                            {
                                continue;
                            }

                            let diagnostic_range = match field_node {
                                Some(field_node) => {
                                    this.translate_to_file_range(field_node, offset)
                                }
                                None => anchor_range,
                            };

                            push_forbidden_field_diagnostic(
                                ctx.diagnostics,
                                Diagnostic {
                                    range: diagnostic_range,
                                    severity: Some(DiagnosticSeverity::ERROR),
                                    message: format!(
                                        "Field '{}' is forbidden on type '{}'{}{}{}",
                                        field_name_str,
                                        type_name,
                                        operation_suffix(scope),
                                        via_fragment_suffix(via_fragment.as_deref()),
                                        rule.reason()
                                            .map(|r| format!(": {}", r))
                                            .unwrap_or_default()
                                    ),
                                    code: Some(NumberOrString::String(
                                        "forbidden_field_selected".to_string(),
                                    )),
                                    data: Some(serde_json::json!({
                                        "scope": "response_key",
                                        "response_key": response_key.as_ref(),
                                        "field_name": field_name_str.as_ref(),
                                        "via_fragment": via_fragment
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

        // Also check type conditions (inline fragments)
        for (response_key, type_fields) in &ctx.type_condition_fields {
            for (type_name, fields) in type_fields {
                for field_name_str in fields {
                    if let Some(rule) = rules.get_forbidden_rule(type_name, field_name_str) {
                        if !scope.allows(rule) {
                            continue;
                        }

                        let origin = fragment_only_origin(ctx, response_key).cloned();
                        if origin.is_some() && rule.applies_to_any_operation() {
                            continue;
                        }

                        let mut via_fragment =
                            origin.as_ref().map(|o| o.fragment.as_ref().to_string());

                        let mut field_node = if origin.is_some() {
                            None
                        } else {
                            find_field_node_in_type_condition(
                                this,
                                node,
                                offset,
                                response_key,
                                type_name,
                                field_name_str,
                            )
                        };
                        if origin.is_none()
                            && field_node.is_none()
                            && let Some((spread_node, fragment_name)) =
                                find_fragment_spread_selecting_field(
                                    this,
                                    node,
                                    offset,
                                    ctx.all_fragments,
                                    response_key,
                                    field_name_str,
                                    Some(type_name),
                                    ctx.response_key_types.get(response_key),
                                    ctx.schema,
                                )
                        {
                            field_node = Some(spread_node);
                            via_fragment = Some(fragment_name);
                        }

                        if origin.is_some() || field_node.is_some() {
                            let anchor_range = match &origin {
                                Some(origin) => {
                                    if nested_selection_ignored(
                                        this,
                                        node,
                                        offset,
                                        ctx,
                                        origin,
                                        definition_range,
                                        IgnoreRule::ForbiddenFields,
                                    ) {
                                        continue;
                                    }
                                    origin.anchor
                                }
                                None => {
                                    let Some(anchor_range) = resolve_anchor_and_check_ignore(
                                        this,
                                        node,
                                        offset,
                                        ctx,
                                        response_key,
                                        definition_range,
                                        None,
                                    ) else {
                                        continue;
                                    };
                                    anchor_range
                                }
                            };

                            // A forbidden field is present, so it is silenced
                            // on itself — the narrowest thing there is, and the
                            // line the diagnostic points at. That holds whether
                            // the field is written here or in the body of a
                            // fragment spread here, which is why the answer
                            // comes from the collected scopes rather than from
                            // a node in this document.
                            if selection_ignored(
                                ctx,
                                response_key,
                                field_name_str,
                                IgnoreRule::ForbiddenFields,
                            ) {
                                continue;
                            }

                            // `field_node` is the field itself when it is
                            // written here, and the spread that merged it in
                            // when it is not. Either is a placement that
                            // covers this finding: the field is the narrowest
                            // thing there is, and a spread is a leaf.
                            if let Some(field_node) = field_node
                                && this.ignore_covers(
                                    field_node,
                                    offset,
                                    IgnoreRule::ForbiddenFields,
                                )
                            {
                                continue;
                            }

                            let diagnostic_range = match field_node {
                                Some(field_node) => {
                                    this.translate_to_file_range(field_node, offset)
                                }
                                None => anchor_range,
                            };

                            push_forbidden_field_diagnostic(
                                ctx.diagnostics,
                                Diagnostic {
                                    range: diagnostic_range,
                                    severity: Some(DiagnosticSeverity::ERROR),
                                    message: format!(
                                        "Field '{}' is forbidden on '... on {}'{}{}{}",
                                        field_name_str,
                                        type_name,
                                        operation_suffix(scope),
                                        via_fragment_suffix(via_fragment.as_deref()),
                                        rule.reason()
                                            .map(|r| format!(": {}", r))
                                            .unwrap_or_default()
                                    ),
                                    code: Some(NumberOrString::String(
                                        "forbidden_field_selected".to_string(),
                                    )),
                                    data: Some(serde_json::json!({
                                        "scope": "response_key",
                                        "response_key": response_key.as_ref(),
                                        "field_name": field_name_str.as_ref(),
                                        "via_fragment": via_fragment
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
}

/// The spread a response key's selections came in through, when the key exists
/// only because a fragment nests it. A key the document declares itself is
/// checked as usual: the fragment's fields merge into that one selection set.
fn fragment_only_origin<'a>(
    ctx: &'a ValidationContext,
    response_key: &str,
) -> Option<&'a crate::diagnostics::FragmentOrigin> {
    if ctx.document_response_keys.contains(response_key) {
        return None;
    }
    ctx.fragment_origins.get(response_key)
}

/// Whether a nested selection's diagnostic is suppressed. The object has no line
/// of its own in this document, so any authored selection the walk passed through
/// to reach it counts: the selection inside the fragment that owns the path
/// (which travels with the fragment, covering every document that spreads it),
/// the spread, or the selection the spread sits in.
/// Whether the selection itself carries an ignore covering `rule`, wherever it
/// was written — in this document, or in the body of a fragment spread here.
fn selection_ignored(
    ctx: &ValidationContext,
    response_key: &str,
    field_name: &str,
    rule: IgnoreRule,
) -> bool {
    ctx.selection_ignores
        .get(&(
            std::sync::Arc::from(response_key),
            std::sync::Arc::from(field_name),
        ))
        .is_some_and(|scope| scope.covers(rule))
}

fn nested_selection_ignored(
    this: &DocumentState,
    node: Node,
    offset: usize,
    ctx: &ValidationContext,
    origin: &crate::diagnostics::FragmentOrigin,
    definition_range: Range,
    rule: IgnoreRule,
) -> bool {
    // A spread applies wide, whichever rule is asking. It is a leaf — there is
    // nothing inside it to annotate — and silencing there covers the operation
    // doing the spreading rather than every operation that spreads the
    // fragment. Spreads inside a fragment are recorded per selection as the
    // walk passes them, since two can feed one key.
    if find_node_for_range(this, node, offset, &origin.anchor)
        .is_some_and(|anchor_node| this.ignore_covers(anchor_node, offset, rule))
    {
        return true;
    }

    // The remaining two placements are both a *parent* of the selection: the
    // object that opens the path inside the fragment, and the selection in this
    // document that holds the spread. They speak for a rule about a field that
    // is not there to annotate, which is required_fields and nothing else. A
    // forbidden field is present, so it is silenced on itself.
    if rule != IgnoreRule::RequiredFields {
        return false;
    }

    if origin.ignored.covers(rule) {
        return true;
    }
    // None means every anchor for that key carries the comment.
    resolve_anchor_and_check_ignore(
        this,
        node,
        offset,
        ctx,
        &origin.spread_parent,
        definition_range,
        Some(rule),
    )
    .is_none()
}

/// ` inside fragment 'X'` when the response key stands for an object nested in a
/// spread fragment, where the missing field has to be added.
fn nested_in_fragment_suffix(fragment: Option<&str>) -> String {
    match fragment {
        Some(name) => format!(" inside fragment '{}'", name),
        None => String::new(),
    }
}

/// `, selected via fragment 'X'` when the field reached this definition through
/// a fragment spread rather than an inline selection.
fn via_fragment_suffix(via_fragment: Option<&str>) -> String {
    match via_fragment {
        Some(name) => format!(", selected via fragment '{}'", name),
        None => String::new(),
    }
}

/// ` in query operations` inside an operation, empty inside a fragment where
/// the enclosing operation is unknown.
fn operation_suffix(scope: RuleScope) -> String {
    match scope.operation_type() {
        Some(op) => format!(" in {} operations", op),
        None => String::new(),
    }
}

/// Range of a definition's name, falling back to the whole definition. Operation
/// names are a direct `name` child; fragment names are nested under
/// `fragment_name`.
fn definition_name_range(this: &DocumentState, node: Node, offset: usize) -> Range {
    let mut cursor = node.walk();
    let name_node = node
        .children(&mut cursor)
        .find(|c| c.kind() == "name")
        .or_else(|| {
            this.find_child_by_kind(node, "fragment_name")
                .and_then(|n| this.find_child_by_kind(n, "name"))
        });

    name_node
        .map(|n| this.translate_to_file_range(n, offset))
        .unwrap_or_else(|| this.translate_to_file_range(node, offset))
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

/// Extract a field node's response key (alias if present, else the field name).
fn field_response_key(this: &DocumentState, field: Node, offset: usize) -> String {
    let components = this.extract_field_components(field);
    if let Some(alias) = components.alias
        && let Some(alias_name) = this.find_child_by_kind(alias, "name")
    {
        return this.get_node_text(alias_name, offset);
    }
    components
        .name
        .map(|n| this.get_node_text(n, offset))
        .unwrap_or_default()
}

/// Walk a dotted response-key path (e.g. "account.billing.subscription") from
/// `node` (an operation_definition or field node) and return the field node
/// reached at the end of the path. Inline fragments are transparent: they do
/// not contribute a path segment, matching how paths are built during
/// validation (see `validate_field`).
fn find_field_node_at_path<'a>(
    this: &DocumentState,
    node: Node<'a>,
    offset: usize,
    segments: &[&str],
) -> Option<Node<'a>> {
    if segments.is_empty() {
        return Some(node);
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "selection_set"
            && let Some(found) = find_segment_in_selection_set(this, child, offset, segments)
        {
            return Some(found);
        }
    }
    None
}

fn find_segment_in_selection_set<'a>(
    this: &DocumentState,
    selection_set: Node<'a>,
    offset: usize,
    segments: &[&str],
) -> Option<Node<'a>> {
    let mut cursor = selection_set.walk();
    for selection in selection_set.children(&mut cursor) {
        let (field_node, inline_node) = match selection.kind() {
            "selection" => (
                this.find_child_by_kind(selection, "field"),
                this.find_child_by_kind(selection, "inline_fragment"),
            ),
            "field" => (Some(selection), None),
            "inline_fragment" => (None, Some(selection)),
            _ => (None, None),
        };

        if let Some(field) = field_node {
            if field_response_key(this, field, offset) == segments[0]
                && let Some(found) = find_field_node_at_path(this, field, offset, &segments[1..])
            {
                return Some(found);
            }
        } else if let Some(inline) = inline_node
            && let Some(inner_set) = this.find_child_by_kind(inline, "selection_set")
            && let Some(found) = find_segment_in_selection_set(this, inner_set, offset, segments)
        {
            return Some(found);
        }
    }
    None
}

/// An inline fragment with no type condition (`... { }`, or one carrying only
/// directives) selects on the enclosing type, so validation records its fields
/// against the enclosing response key exactly as if the braces were not there.
/// The lookups that go back for a node to point at have to see through it the
/// same way.
///
/// Returns the selections of `selection_set` with every such inline fragment
/// flattened away. Inline fragments that do carry a type condition are returned
/// as they are: their fields are tracked separately, under the condition.
fn transparent_selections<'a>(this: &DocumentState, selection_set: Node<'a>) -> Vec<Node<'a>> {
    let mut out = Vec::new();
    collect_transparent_selections(this, selection_set, &mut out, 0);
    out
}

/// Nesting of condition-less inline fragments this flattening follows. Nothing
/// legitimate stacks them, so the limit only keeps a pathological document from
/// reaching the recursion for every level of it.
const MAX_TRANSPARENT_DEPTH: usize = 64;

fn collect_transparent_selections<'a>(
    this: &DocumentState,
    selection_set: Node<'a>,
    out: &mut Vec<Node<'a>>,
    depth: usize,
) {
    if depth > MAX_TRANSPARENT_DEPTH {
        return;
    }
    let mut cursor = selection_set.walk();
    for selection in selection_set.children(&mut cursor) {
        for kind in ["field", "inline_fragment", "fragment_spread"] {
            let payload = if selection.kind() == "selection" {
                this.find_child_by_kind(selection, kind)
            } else if selection.kind() == kind {
                Some(selection)
            } else {
                None
            };
            let Some(payload) = payload else {
                continue;
            };

            if payload.kind() == "inline_fragment"
                && this.find_child_by_kind(payload, "type_condition").is_none()
            {
                if let Some(inner) = this.find_child_by_kind(payload, "selection_set") {
                    collect_transparent_selections(this, inner, out, depth + 1);
                }
            } else {
                out.push(payload);
            }
        }
    }
}

/// Find a field named `field_name` selected directly under `selection_set`,
/// looking through any condition-less inline fragment in the way.
fn find_direct_field_child<'a>(
    this: &DocumentState,
    selection_set: Node<'a>,
    offset: usize,
    field_name: &str,
) -> Option<Node<'a>> {
    for field in transparent_selections(this, selection_set) {
        if field.kind() != "field" {
            continue;
        }
        let name = this
            .extract_field_components(field)
            .name
            .map(|n| this.get_node_text(n, offset))
            .unwrap_or_default();
        if name == field_name {
            return Some(field);
        }
    }
    None
}

/// Find the field node `field_name` selected directly under the field at the
/// given response-key path. `response_key` is the full dotted path from the
/// operation root.
fn find_field_node_by_name<'a>(
    this: &DocumentState,
    node: Node<'a>,
    offset: usize,
    response_key: &str,
    field_name: &str,
) -> Option<Node<'a>> {
    let segments: Vec<&str> = response_key.split('.').collect();
    let parent = find_field_node_at_path(this, node, offset, &segments)?;
    let selection_set = this.find_child_by_kind(parent, "selection_set")?;
    find_direct_field_child(this, selection_set, offset, field_name)
}

/// Fields selected inside a spread fragment are merged into the enclosing
/// response key, so a forbidden field can have no field node in this definition
/// to point at. Find the spread that pulled the field in and report there
/// instead, naming the fragment so the source of the field is obvious.
///
/// `type_condition` is set when the field was tracked under `... on X` rather
/// than directly under the response key.
#[allow(clippy::too_many_arguments)]
fn find_fragment_spread_selecting_field<'a>(
    this: &DocumentState,
    node: Node<'a>,
    offset: usize,
    all_fragments: &[crate::completion::FragmentCompletionInfo],
    response_key: &str,
    field_name: &str,
    type_condition: Option<&str>,
    key_type: Option<&ExtendedType>,
    schema: &apollo_compiler::validation::Valid<apollo_compiler::Schema>,
) -> Option<(Node<'a>, String)> {
    let segments: Vec<&str> = response_key.split('.').collect();
    let parent = find_field_node_at_path(this, node, offset, &segments)?;
    let selection_set = this.find_child_by_kind(parent, "selection_set")?;
    find_spread_selecting_field(
        this,
        selection_set,
        offset,
        all_fragments,
        field_name,
        type_condition,
        None,
        key_type,
        schema,
    )
}

/// Walk a selection set looking for a fragment spread that contributes
/// `field_name` to the enclosing response key. `current_type_condition` tracks
/// the inline fragment we are inside of, mirroring how the spread's fields were
/// recorded during validation.
#[allow(clippy::too_many_arguments)]
fn find_spread_selecting_field<'a>(
    this: &DocumentState,
    selection_set: Node<'a>,
    offset: usize,
    all_fragments: &[crate::completion::FragmentCompletionInfo],
    field_name: &str,
    type_condition: Option<&str>,
    current_type_condition: Option<&str>,
    key_type: Option<&ExtendedType>,
    schema: &apollo_compiler::validation::Valid<apollo_compiler::Schema>,
) -> Option<(Node<'a>, String)> {
    let mut cursor = selection_set.walk();
    for selection in selection_set.children(&mut cursor) {
        let (spread_node, inline_node) = match selection.kind() {
            "selection" => (
                this.find_child_by_kind(selection, "fragment_spread"),
                this.find_child_by_kind(selection, "inline_fragment"),
            ),
            "fragment_spread" => (Some(selection), None),
            "inline_fragment" => (None, Some(selection)),
            _ => (None, None),
        };

        if let Some(spread) = spread_node {
            let Some(name_node) = this
                .find_child_by_kind(spread, "fragment_name")
                .and_then(|n| this.find_child_by_kind(n, "name"))
            else {
                continue;
            };
            let fragment_name = this.get_node_text(name_node, offset);

            // Where a spread's fields land depends on the inline fragment it
            // sits in and on its own type condition, which narrows an abstract
            // response key exactly as `... on X` would. Ask the same routine
            // that recorded them.
            if crate::diagnostics::fragments::spread_contributes_field_under(
                this,
                schema,
                all_fragments,
                &fragment_name,
                field_name,
                type_condition,
                current_type_condition,
                key_type,
                &mut ahash::AHashSet::default(),
                0,
            ) {
                return Some((name_node, fragment_name));
            }
        } else if let Some(inline) = inline_node
            && let Some(inner_set) = this.find_child_by_kind(inline, "selection_set")
        {
            let inline_tc = this
                .find_child_by_kind(inline, "type_condition")
                .and_then(|tc| this.find_child_by_kind(tc, "named_type"))
                .map(|nt| this.get_node_text(nt, offset));
            let next_tc = inline_tc.as_deref().or(current_type_condition);

            if let Some(found) = find_spread_selecting_field(
                this,
                inner_set,
                offset,
                all_fragments,
                field_name,
                type_condition,
                next_tc,
                key_type,
                schema,
            ) {
                return Some(found);
            }
        }
    }
    None
}

/// Find the field node `field_name` selected inside an inline fragment
/// `... on type_name` under the field at the given response-key path.
fn find_field_node_in_type_condition<'a>(
    this: &DocumentState,
    node: Node<'a>,
    offset: usize,
    response_key: &str,
    type_name: &str,
    field_name: &str,
) -> Option<Node<'a>> {
    let segments: Vec<&str> = response_key.split('.').collect();
    let parent = find_field_node_at_path(this, node, offset, &segments)?;
    let selection_set = this.find_child_by_kind(parent, "selection_set")?;

    for inline in transparent_selections(this, selection_set) {
        if inline.kind() != "inline_fragment" {
            continue;
        }
        if let Some(tc) = this.find_child_by_kind(inline, "type_condition")
            && let Some(name_node) = this.find_child_by_kind(tc, "named_type")
            && this.get_node_text(name_node, offset) == type_name
            && let Some(frag_selection_set) = this.find_child_by_kind(inline, "selection_set")
            && let Some(found) =
                find_direct_field_child(this, frag_selection_set, offset, field_name)
        {
            return Some(found);
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
    definition_range: Range,
    // Which rule an ignore comment on the anchor may silence, if any. None asks
    // only for a range to point at: a forbidden field is silenced on itself,
    // never from the object holding it.
    suppress_on: Option<IgnoreRule>,
) -> Option<Range> {
    if let Some(ranges) = ctx.response_key_anchor_ranges.get(response_key) {
        let mut first_non_ignored = None;
        let mut all_ignored = true;

        for anchor_range in ranges {
            if let Some(rule) = suppress_on
                && let Some(anchor_node) = find_node_for_range(this, node, offset, anchor_range)
                && this.ignore_covers(anchor_node, offset, rule)
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

        return Some(first_non_ignored.unwrap_or(definition_range));
    }

    // Fallback to the enclosing definition
    if let Some(rule) = suppress_on
        && let Some(anchor_node) = find_node_for_range(this, node, offset, &definition_range)
        && this.ignore_covers(anchor_node, offset, rule)
    {
        return None;
    }

    Some(definition_range)
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
