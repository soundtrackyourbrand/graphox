use graphql_rust::{
    config::{GlobPattern, ProjectConfig, SchemaSource},
    Config,
};
use std::fs;
use tempfile::tempdir;

// LSP tests use these
use tower_lsp::LspService;
use tower_lsp::jsonrpc::Request;
use tower_lsp::lsp_types::*;

// Basic analysis test: shallow duplicate fields
#[test]
#[ntest::timeout(5000)]
fn test_shallow_duplicate_fields_check() {
    let dir = tempdir().unwrap();
    let base = dir.path();

    fs::write(base.join("package.json"), "{}").unwrap();
    fs::write(
        base.join("schema.graphql"),
        "type Query { me: User } type User { id: ID name: String }",
    )
    .unwrap();

    // Query with duplicate shallow fields
    fs::write(base.join("q.graphql"), "query { me { id id name } }").unwrap();

    let config = Config {
        base_dir: base.to_path_buf(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None,
        }],
        rules: Some(Default::default()),
        ..Default::default()
    };

    // Enable the rule
    let mut cfg = config;
    if let Some(rules) = &mut cfg.rules {
        rules.no_duplicate_fields = Some(true);
    }

    // Run workspace scan and checks similar to check command
    let meta = graphql_rust::engine::Engine::scan_workspace(&cfg, |_, _| {});
    let docs = meta.documents;
    assert!(docs.values().any(|d| d.operations().len() >= 0));

    // Run project check to collect diagnostics and assert diagnostic message and range
    let mut found = false;
    for (_path, doc) in docs {
        let schema_text = std::fs::read_to_string(base.join("schema.graphql")).unwrap();
        let schema = apollo_compiler::Schema::parse(&schema_text, "schema.graphql").unwrap();
        let valid = schema.validate().unwrap();
        let diags = doc.get_semantic_diagnostics(&valid, &[], None, Some(&cfg), false, true);
        for d in diags {
            if let Some(t) = &d.code {
                if let tower_lsp::lsp_types::NumberOrString::String(s) = t {
                    if s == "no_duplicate_fields" {
                        // Verify message and range for the duplicate `id` occurrence
                        assert_eq!(d.message, "Duplicate field 'id' in selection set");
                        let expected_range = Range::new(Position::new(0, 13), Position::new(0, 15));
                        assert_eq!(
                            d.range, expected_range,
                            "Diagnostic range should point to the duplicate 'id' name"
                        );
                        found = true;
                    }
                }
            }
        }
    }

    assert!(found, "Expected duplicate field diagnostic to be reported");
}

// Canonicalization test: arg order variations should be reported
#[test]
#[ntest::timeout(5000)]
fn test_duplicate_fields_with_different_arg_order_are_reported() {
    let dir = tempdir().unwrap();
    let base = dir.path();

    fs::write(base.join("package.json"), "{}").unwrap();
    fs::write(
        base.join("schema.graphql"),
        "type Query { me(id: ID, other: Int): User } type User { id: ID name: String }",
    )
    .unwrap();

    // Same response key, same args but different order -> should be reported as duplicate
    fs::write(
        base.join("q.graphql"),
        "query { me(id: $id, other: 2) { id } me(other: 2, id: $id) { id } }",
    )
    .unwrap();

    let config = Config {
        base_dir: base.to_path_buf(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None,
        }],
        rules: Some(Default::default()),
        ..Default::default()
    };

    // Enable the rule
    let mut cfg = config;
    if let Some(rules) = &mut cfg.rules {
        rules.no_duplicate_fields = Some(true);
    }

    let meta = graphql_rust::engine::Engine::scan_workspace(&cfg, |_, _| {});
    let docs = meta.documents;

    // Run validation and ensure duplicate diagnostics are emitted for arg order variations
    let mut found = false;
    for (_path, doc) in docs {
        let schema_text = std::fs::read_to_string(base.join("schema.graphql")).unwrap();
        let schema = apollo_compiler::Schema::parse(&schema_text, "schema.graphql").unwrap();
        let valid = schema.validate().unwrap();
        let diags = doc.get_semantic_diagnostics(&valid, &[], None, Some(&cfg), false, true);
        for d in diags {
            if let Some(tower_lsp::lsp_types::NumberOrString::String(s)) = &d.code {
                if s == "no_duplicate_fields" {
                    // Verify message and range for the duplicate `me` occurrence
                    assert_eq!(d.message, "Duplicate field 'me' in selection set");
                    let expected_range = Range::new(Position::new(0, 37), Position::new(0, 39));
                    assert_eq!(d.range, expected_range, "Diagnostic range should point to the duplicate 'me' name");
                    found = true;
                }
            }
        }
    }
    assert!(found, "Expected duplicate field diagnostic to be reported for arg order variations");
}

// Alias handling tests
#[test]
#[ntest::timeout(5000)]
fn test_duplicate_fields_with_alias_handling() {
    let dir = tempdir().unwrap();
    let base = dir.path();

    fs::write(base.join("package.json"), "{}").unwrap();
    fs::write(
        base.join("schema.graphql"),
        "type Query { me: User } type User { id: ID name: String }",
    )
    .unwrap();

    // Query where alias and name coexist (different response keys) -> should NOT trigger
    fs::write(
        base.join("q_aliases.graphql"),
        "query { me { a: id id name } }",
    )
    .unwrap();

    // Query with duplicated alias (same response key repeated) -> should trigger
    fs::write(
        base.join("q_duplicate_alias.graphql"),
        "query { me { a: id a: id } }",
    )
    .unwrap();

    let config = Config {
        base_dir: base.to_path_buf(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None,
        }],
        rules: Some(Default::default()),
        ..Default::default()
    };

    // Enable the rule
    let mut cfg = config;
    if let Some(rules) = &mut cfg.rules {
        rules.no_duplicate_fields = Some(true);
    }

    let meta = graphql_rust::engine::Engine::scan_workspace(&cfg, |_, _| {});
    let docs = meta.documents;

    let schema_text = std::fs::read_to_string(base.join("schema.graphql")).unwrap();
    let schema = apollo_compiler::Schema::parse(&schema_text, "schema.graphql").unwrap();
    let valid = schema.validate().unwrap();

    let mut found_duplicate = false;
    let mut found_unexpected = false;

    for (path, doc) in docs {
        let diags = doc.get_semantic_diagnostics(&valid, &[], None, Some(&cfg), false, true);

        if path.ends_with("q_duplicate_alias.graphql") {
            // Expect a diagnostic for the duplicated alias `a`
            if let Some(d) = diags.iter().find(|d| matches!(&d.code, Some(tower_lsp::lsp_types::NumberOrString::String(s)) if s == "no_duplicate_fields")) {
                assert_eq!(d.message, "Duplicate field 'a' in selection set");
                let expected_range = Range::new(Position::new(0, 19), Position::new(0, 20));
                assert_eq!(d.range, expected_range, "Diagnostic range should point to the duplicate alias 'a'");
                found_duplicate = true;
            }
        }

        if path.ends_with("q_aliases.graphql") {
            // Should not report duplicates when aliases produce unique response keys
            let has_dup = diags.iter().any(|d| match &d.code {
                Some(tower_lsp::lsp_types::NumberOrString::String(s)) => s == "no_duplicate_fields",
                _ => false,
            });
            if has_dup {
                found_unexpected = true;
            }
        }
    }

    assert!(found_duplicate, "Expected duplicate alias diagnostic to be reported");
    assert!(!found_unexpected, "Did not expect a duplicate diagnostic for aliased+name case");
}

// Alias collisions: alias name equals an unaliased field -> should trigger
#[test]
#[ntest::timeout(5000)]
fn test_alias_collision_triggers_duplicate() {
    let dir = tempdir().unwrap();
    let base = dir.path();

    fs::write(base.join("package.json"), "{}").unwrap();
    fs::write(
        base.join("schema.graphql"),
        "type Query { me: User } type User { id: ID name: String }",
    )
    .unwrap();

    // Alias 'id' collides with unaliased 'id' field -> should trigger duplicate diagnostic
    fs::write(
        base.join("q_collision.graphql"),
        "query { me { id name id: name } }",
    )
    .unwrap();

    let config = Config {
        base_dir: base.to_path_buf(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None,
        }],
        rules: Some(Default::default()),
        ..Default::default()
    };

    // Enable the rule
    let mut cfg = config;
    if let Some(rules) = &mut cfg.rules {
        rules.no_duplicate_fields = Some(true);
    }

    let meta = graphql_rust::engine::Engine::scan_workspace(&cfg, |_, _| {});
    let docs = meta.documents;

    let schema_text = std::fs::read_to_string(base.join("schema.graphql")).unwrap();
    let schema = apollo_compiler::Schema::parse(&schema_text, "schema.graphql").unwrap();
    let valid = schema.validate().unwrap();

    let mut found_collision = false;

    for (path, doc) in docs {
        let diags = doc.get_semantic_diagnostics(&valid, &[], None, Some(&cfg), false, true);
        let has_dup = diags.iter().any(|d| match &d.code {
            Some(tower_lsp::lsp_types::NumberOrString::String(s)) => s == "no_duplicate_fields",
            _ => false,
        });

        if path.ends_with("q_collision.graphql") {
            if has_dup {
                found_collision = true;
            }
        }
    }

    assert!(
        found_collision,
        "Expected alias collision to trigger duplicate diagnostic"
    );
}

// Tests involving fragments and inline fragments
#[test]
#[ntest::timeout(5000)]
fn test_duplicate_fields_with_fragments_and_inline_fragments() {
    let dir = tempdir().unwrap();
    let base = dir.path();

    fs::write(base.join("package.json"), "{}").unwrap();
    fs::write(
        base.join("schema.graphql"),
        "type Query { me: User } type User { id: ID name: String friends(limit: Int): [User] }",
    )
    .unwrap();

    // A: duplicate inside the same inline fragment selection set -> should trigger
    fs::write(
        base.join("q_inline_dup.graphql"),
        "query { me { ... on User { id id } } }",
    )
    .unwrap();

    // B: duplicate across inline fragment and sibling -> should NOT trigger (shallow-only)
    fs::write(
        base.join("q_inline_cross.graphql"),
        "query { me { ... on User { id } id } }",
    )
    .unwrap();

    // C: duplicate across fragment spread and sibling -> should NOT trigger
    fs::write(base.join("frag_def.graphql"), "fragment F on User { id }").unwrap();
    fs::write(
        base.join("q_frag_spread.graphql"),
        "query { me { ...F id } }",
    )
    .unwrap();

    // D: same response key with different args -> should trigger
    fs::write(
        base.join("q_args_conflict.graphql"),
        "query { me { friends(limit: 5) { id } friends(limit: 10) { id } } }",
    )
    .unwrap();

    // E: same response key with identical args/selection -> should NOT trigger
    fs::write(
        base.join("q_args_same.graphql"),
        "query { me { friends(limit: 5) { id } friends(limit: 5) { id } } }",
    )
    .unwrap();

    let config = Config {
        base_dir: base.to_path_buf(),
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: None,
        }],
        rules: Some(Default::default()),
        ..Default::default()
    };

    // Enable the rule
    let mut cfg = config;
    if let Some(rules) = &mut cfg.rules {
        rules.no_duplicate_fields = Some(true);
    }

    let meta = graphql_rust::engine::Engine::scan_workspace(&cfg, |_, _| {});
    let docs = meta.documents;

    let schema_text = std::fs::read_to_string(base.join("schema.graphql")).unwrap();
    let schema = apollo_compiler::Schema::parse(&schema_text, "schema.graphql").unwrap();
    let valid = schema.validate().unwrap();

    let mut a_flag = false;
    let mut b_flag = false;
    let mut c_flag = false;
    let mut d_flag = false;
    let mut e_flag = false;

    for (path, doc) in docs {
        let diags = doc.get_semantic_diagnostics(&valid, &[], None, Some(&cfg), false, true);

        if path.ends_with("q_inline_dup.graphql") {
            // Expect duplicate inside inline fragment: check message & range
            if let Some(d) = diags.iter().find(|d| matches!(&d.code, Some(tower_lsp::lsp_types::NumberOrString::String(s)) if s == "no_duplicate_fields")) {
                assert_eq!(d.message, "Duplicate field 'id' in selection set");
                let expected_range = Range::new(Position::new(0, 25), Position::new(0, 27));
                assert_eq!(d.range, expected_range, "Diagnostic range should point to the duplicate 'id' inside inline fragment");
                a_flag = true;
            }
        }

        if path.ends_with("q_inline_cross.graphql") {
            if diags.iter().any(|d| matches!(&d.code, Some(tower_lsp::lsp_types::NumberOrString::String(s)) if s == "no_duplicate_fields")) {
                b_flag = true;
            }
        }

        if path.ends_with("q_frag_spread.graphql") {
            if diags.iter().any(|d| matches!(&d.code, Some(tower_lsp::lsp_types::NumberOrString::String(s)) if s == "no_duplicate_fields")) {
                c_flag = true;
            }
        }

        if path.ends_with("q_args_conflict.graphql") {
            // Expect args conflict diagnostic and verify message/range
            if let Some(d) = diags.iter().find(|d| matches!(&d.code, Some(tower_lsp::lsp_types::NumberOrString::String(s)) if s == "no_duplicate_fields")) {
                assert_eq!(d.message, "Duplicate field 'friends' in selection set");
                let expected_range = Range::new(Position::new(0, 37), Position::new(0, 44));
                assert_eq!(d.range, expected_range, "Diagnostic range should point to the duplicate 'friends' occurrence");
                d_flag = true;
            }
        }

        if path.ends_with("q_args_same.graphql") {
            if diags.iter().any(|d| matches!(&d.code, Some(tower_lsp::lsp_types::NumberOrString::String(s)) if s == "no_duplicate_fields")) {
                e_flag = true;
            }
        }
    }

    assert!(
        a_flag,
        "Expected duplicate inside inline fragment to be reported"
    );
    assert!(
        !b_flag,
        "Did not expect cross-inline-fragment duplicate to be reported"
    );
    assert!(
        !c_flag,
        "Did not expect fragment-spread duplicate to be reported"
    );
    assert!(d_flag, "Expected args-conflict duplicate to be reported");
    assert!(
        !e_flag,
        "Did not expect identical args duplicate to be reported"
    );
}

// LSP integration test that also applies the code action via DocumentState
#[tokio::test]
async fn test_alias_allowed_and_duplicate_code_action_removes_later() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    fs::write(base_dir.join("package.json"), "{}").unwrap();
    let schema_path = base_dir.join("schema.graphql");
    fs::write(&schema_path, "type Query { me: User } type User { id: ID name: String }").unwrap();

    let config = Config {
        projects: vec![ProjectConfig {
            schema: SchemaSource::Single("schema.graphql".to_string()),
            include: GlobPattern::Single("**/*.graphql".to_string()),
            exclude: None,
            output_dir: None,
            import: None,
            generate_permissions: None,
            codegen: Some(false),
        }],
        enable_schema_cache: Some(true),
        base_dir: base_dir.to_path_buf(),
        lsp_automatic_codegen: Some(false),
        ..Config::new_empty()
    };

    let (mut service, _) = LspService::new(|client| Backend::new(client, config));

    service
        .call(
            Request::build("initialize")
                .params(serde_json::to_value(InitializeParams::default()).unwrap())
                .id(0)
                .finish(),
        )
        .await
        .unwrap()
        .unwrap();
    service
        .call(
            Request::build("initialized")
                .params(serde_json::json!({}))
                .finish(),
        )
        .await
        .unwrap();

    // Create a document where aliases are used uniquely and should NOT trigger duplicate diagnostic
    let q_alias_path = base_dir.join("aliases_ok.graphql");
    let q_alias_text = "query { me { a: id b: id name } }"; // aliases a and b make response keys unique
    fs::write(&q_alias_path, q_alias_text).unwrap();
    let q_alias_path = std::fs::canonicalize(q_alias_path).unwrap();
    let q_alias_uri = Url::from_file_path(&q_alias_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: q_alias_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: q_alias_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Create a document with a true duplicate (no alias differentiation)
    let dup_path = base_dir.join("dup_action.graphql");
    let dup_text = "query { me { id id name } }";
    fs::write(&dup_path, dup_text).unwrap();
    let dup_path = std::fs::canonicalize(dup_path).unwrap();
    let dup_uri = Url::from_file_path(&dup_path).unwrap();

    service
        .call(
            Request::build("textDocument/didOpen")
                .params(
                    serde_json::to_value(DidOpenTextDocumentParams {
                        text_document: TextDocumentItem {
                            uri: dup_uri.clone(),
                            language_id: "graphql".to_string(),
                            version: 1,
                            text: dup_text.to_string(),
                        },
                    })
                    .unwrap(),
                )
                .finish(),
        )
        .await
        .unwrap();

    // Request code actions for the alias doc to ensure none report duplicate-field
    let params_alias = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: q_alias_uri.clone() },
        range: Range::new(Position::new(0, 0), Position::new(0, q_alias_text.len() as u32)),
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let request_alias = Request::build("textDocument/codeAction")
        .id(1)
        .params(serde_json::to_value(&params_alias).unwrap())
        .finish();
    let response_alias = service.call(request_alias).await.unwrap().unwrap();
    let result_alias: Option<CodeActionResponse> = serde_json::from_value(response_alias.result().unwrap().clone()).unwrap();
    let actions_alias = result_alias.expect("Expected actions for alias file");
    // Ensure none of the returned actions are 'Remove duplicate field'
    assert!(actions_alias.iter().all(|a| match a { CodeActionOrCommand::CodeAction(ca) => ca.title != "Remove duplicate field", _ => true }));

    // Construct diagnostic pointing at duplicated `id` in dup_action.graphql
    let dup_diag = Diagnostic {
        range: Range::new(Position::new(0, 13), Position::new(0, 15)),
        message: "Duplicate field 'id' in selection set".to_string(),
        code: Some(NumberOrString::String("no_duplicate_fields".to_string())),
        data: Some(serde_json::json!({"response_key":"id","args":"","selection":""})),
        ..Default::default()
    };

    let params_dup = CodeActionParams {
        text_document: TextDocumentIdentifier { uri: dup_uri.clone() },
        range: dup_diag.range,
        context: CodeActionContext {
            diagnostics: vec![dup_diag.clone()],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    };

    let request_dup = Request::build("textDocument/codeAction")
        .id(2)
        .params(serde_json::to_value(&params_dup).unwrap())
        .finish();
    let response_dup = service.call(request_dup).await.unwrap().unwrap();
    let result_dup: Option<CodeActionResponse> = serde_json::from_value(response_dup.result().unwrap().clone()).unwrap();
    let actions_dup = result_dup.expect("Expected actions for dup file");

    let dup_action = actions_dup
        .iter()
        .find(|a| match a { CodeActionOrCommand::CodeAction(ca) => ca.title == "Remove duplicate field", _ => false })
        .expect("Expected 'Remove duplicate field' action");

    if let CodeActionOrCommand::CodeAction(action) = dup_action {
        let edit = action.edit.as_ref().unwrap();
        let changes = edit.changes.as_ref().unwrap();
        let edits = &changes[&dup_uri];

        // Apply the edits to the content using a document-aware approach: create a DocumentState and apply
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(tree_sitter_graphql::LANGUAGE.into()).unwrap();
        let mut doc = graphql_rust::DocumentState::new(dup_uri.clone(), dup_text, parser);

        if let Some(text_edits) = &edits.get(0) {
            // Each TextEdit produced by our code action should be applicable via apply_change
            let t = TextDocumentContentChangeEvent::new(text_edits.range, None, text_edits.new_text.clone());
            let mut parser2 = tree_sitter::Parser::new();
            parser2.set_language(tree_sitter_graphql::LANGUAGE.into()).unwrap();
            doc.apply_change(&t, &mut parser2, 2);

            // Ensure resulting document still parses
            let mut apollo_parser = apollo_compiler::parser::Parser::new();
            let parse_res = apollo_parser.parse_ast(doc.rope.to_string(), "dup_action.graphql");
            assert!(parse_res.is_ok() || matches!(parse_res, Err(apollo_compiler::validation::WithErrors { partial: _, .. })), "Resulting document should parse or partially parse");

            // The duplicate-field code action should remove the duplicated field and
            // produce the following resulting content (one `id` remains):
            let expected = "query { me { id name } }";
            assert_eq!(doc.rope.to_string(), expected);
        } else {
            panic!("No edits produced by duplicate-field action");
        }
    } else {
        panic!("Expected CodeAction");
    }
}
