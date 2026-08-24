use ahash::AHashMap;
use graphox::Config;
use graphox::config::{ForbiddenFieldRule, RulesConfig};
use graphox::features::diagnostics::DocumentDiagnostics;
use tower_lsp_server::ls_types::*;

use crate::support::{
    assert_diag_range_equals, assert_diagnostic_severity, assert_diagnostic_with_message,
    assert_diagnostics_count, assert_no_diagnostics, create_doc, fixtures,
};

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_always_true() {
    let text = r#"
        query GetUsers {
            users {
                id
                name
                password
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert("password".to_string(), ForbiddenFieldRule::new_always(true));

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Field 'password' is forbidden on type 'User'",
    );
    assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
    assert_diag_range_equals(d, &crate::support::range_for_token(&doc, text, "password"));
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_always_false() {
    let text = r#"
        query GetUsers {
            users {
                id
                name
                password
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "password".to_string(),
        ForbiddenFieldRule::new_always(false),
    );

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_specific_operation_mutation() {
    let text = r#"
        query GetUsers {
            users {
                id
                name
                password
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Forbidden ONLY in mutations
    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "password".to_string(),
        ForbiddenFieldRule::new_operations(vec!["mutation".to_string()]),
    );

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    // Should NOT error in a query
    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_ignored_with_inline_comment() {
    let text = r#"
        query GetUsers {
            users { # graphox-ignore
                id
                password
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert("password".to_string(), ForbiddenFieldRule::new_always(true));

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_with_reason() {
    let text = r#"
        query GetUsers {
            users {
                id
                name
                password
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "password".to_string(),
        ForbiddenFieldRule::new_always(true)
            .with_reason("Passwords must not be fetched directly".to_string()),
    );

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    assert_diagnostic_with_message(
        &diagnostics,
        "Field 'password' is forbidden on type 'User' in query operations: Passwords must not be fetched directly",
    );
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_type_specific() {
    let text = r#"
        query GetData {
            users {
                name
            }
            posts {
                title
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let yaml = r#"
      forbidden_fields:
        User:
          name: true
    "#;
    let yaml_docs = yaml_rust2::YamlLoader::load_from_str(yaml).unwrap();
    let rules = RulesConfig::from_yaml(&yaml_docs[0]).unwrap();

    let config = Config::default().with_rules(rules);

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    // Should error on User.name but NOT on Post.title (even if both were named 'name')
    assert_diagnostics_count(&diagnostics, 1);
    assert_diagnostic_with_message(&diagnostics, "Field 'name' is forbidden on type 'User'");
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_type_override() {
    let text = r#"
        query GetUsers {
            users {
                password
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Global forbidden, but allowed on User
    let yaml = r#"
      forbidden_fields:
        password: true
        User:
          password: false
    "#;
    let yaml_docs = yaml_rust2::YamlLoader::load_from_str(yaml).unwrap();
    let rules = RulesConfig::from_yaml(&yaml_docs[0]).unwrap();

    let config = Config::default().with_rules(rules);

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_same_response_key_different_types() {
    // The response key `subscription` is reached via two paths that resolve to
    // different types. A type-specific forbidden rule on `ZoneSubscription`
    // resolves against that type, independent of the `AccountSubscription`
    // reached via the other path that shares the leaf response key.
    let text = r#"
        query Combo {
            soundZone {
                id
                subscription {
                    state
                }
            }
            account {
                id
                billing {
                    subscription {
                        billingCycle
                    }
                }
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let yaml = r#"
      forbidden_fields:
        ZoneSubscription:
          state: true
    "#;
    let yaml_docs = yaml_rust2::YamlLoader::load_from_str(yaml).unwrap();
    let rules = RulesConfig::from_yaml(&yaml_docs[0]).unwrap();

    let config = Config::default().with_rules(rules);

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::colliding_response_key_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    // Exactly one diagnostic: `state` forbidden on ZoneSubscription, anchored at
    // the `state` selection under the zone subscription (not the account path,
    // which selects `billingCycle` and carries no forbidden rule).
    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Field 'state' is forbidden on type 'ZoneSubscription'",
    );
    assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
    assert_diag_range_equals(d, &crate::support::range_for_token(&doc, text, "state"));
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_nested_inside_fragment_definition() {
    // Same gap as `required_fields`: a nested selection inside a fragment body
    // must be checked, and the diagnostic lands on the fragment's own selection.
    let text = r#"
        fragment PostWithAuthor on Post {
            id
            author {
                id
                password
            }
        }

        query GetPosts {
            posts {
                ...PostWithAuthor
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert("password".to_string(), ForbiddenFieldRule::new_always(true));

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Field 'password' is forbidden on type 'User'",
    );
    assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
    assert_diag_range_equals(d, &crate::support::range_for_token(&doc, text, "password"));
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_operation_scoped_rule_checked_at_the_spread() {
    // The rule cannot be evaluated inside the fragment, where the operation
    // type is unknown, so it is evaluated at the spread and reported there.
    let text = r#"
        fragment PostWithAuthor on Post {
            id
            author {
                id
                password
            }
        }

        query GetPosts {
            posts {
                ...PostWithAuthor
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "password".to_string(),
        ForbiddenFieldRule::new_operations(vec!["query".to_string()]),
    );

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Field 'password' is forbidden on type 'User' in query operations, selected via fragment 'PostWithAuthor'",
    );
    assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
    assert_diag_range_equals(
        d,
        &crate::support::range_for_token(&doc, text, "PostWithAuthor"),
    );
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_selected_via_fragment_spread() {
    // A fragment's top-level fields are merged into the response key it is
    // spread under, so the rule must fire even though the field itself lives in
    // another definition. The spread is the anchor.
    let text = r#"
        fragment PostFields on Post {
            id
            secretField
        }

        query GetPosts {
            posts {
                ...PostFields
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "secretField".to_string(),
        ForbiddenFieldRule::new_always(true),
    );

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Field 'secretField' is forbidden on type 'Post' in query operations, selected via fragment 'PostFields'",
    );
    assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
    assert_diag_range_equals(
        d,
        &crate::support::range_for_token(&doc, text, "PostFields"),
    );
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_via_fragment_spread_only_in_matching_operation() {
    // The same fragment is spread by a query and a subscription. An
    // operation-scoped rule must fire on the subscription only.
    let text = r#"
        fragment UserFields on User {
            id
            username
        }

        query GetUsers {
            users {
                ...UserFields
            }
        }

        subscription OnUserAdded {
            userAdded {
                ...UserFields
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "username".to_string(),
        ForbiddenFieldRule::new_operations(vec!["subscription".to_string()]),
    );

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_subscription_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Field 'username' is forbidden on type 'User' in subscription operations, selected via fragment 'UserFields'",
    );
    // The last spread is the one inside the subscription.
    assert_diag_range_equals(
        d,
        &crate::support::range_for_token(&doc, text, "UserFields"),
    );
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_via_nested_fragment_spread() {
    // The field arrives through a chain of spreads; the outermost spread in the
    // operation is the anchor.
    let text = r#"
        fragment PostSecret on Post {
            secretField
        }

        fragment PostFields on Post {
            id
            ...PostSecret
        }

        query GetPosts {
            posts {
                ...PostFields
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "secretField".to_string(),
        ForbiddenFieldRule::new_always(true),
    );

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Field 'secretField' is forbidden on type 'Post' in query operations, selected via fragment 'PostFields'",
    );
    assert_diag_range_equals(
        d,
        &crate::support::range_for_token(&doc, text, "PostFields"),
    );
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_via_fragment_spread_inside_inline_fragment() {
    let text = r#"
        fragment AuthorFields on User {
            id
            password
        }

        query GetPosts {
            posts {
                author {
                    ... on User {
                        ...AuthorFields
                    }
                }
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert("password".to_string(), ForbiddenFieldRule::new_always(true));

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Field 'password' is forbidden on '... on User' in query operations, selected via fragment 'AuthorFields'",
    );
    assert_diag_range_equals(
        d,
        &crate::support::range_for_token(&doc, text, "AuthorFields"),
    );
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_via_fragment_spread_ignored_with_inline_comment() {
    let text = r#"
        fragment PostFields on Post {
            id
            secretField
        }

        query GetPosts {
            posts { # graphox-ignore
                ...PostFields
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "secretField".to_string(),
        ForbiddenFieldRule::new_always(true),
    );

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_nested_inside_spread_fragment_only_where_it_applies() {
    // One fragment, two consumers. The nested `password` is forbidden in the
    // subscription and fine in the query, which is why the rule has to be
    // evaluated per spread rather than inside the fragment.
    let text = r#"
        fragment PostWithAuthor on Post {
            id
            author {
                id
                password
            }
        }

        query GetPosts {
            posts {
                ...PostWithAuthor
            }
        }

        subscription OnPost {
            postAdded {
                ...PostWithAuthor
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "password".to_string(),
        ForbiddenFieldRule::new_operations(vec!["subscription".to_string()]),
    );

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::post_subscription_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Field 'password' is forbidden on type 'User' in subscription operations, selected via fragment 'PostWithAuthor'",
    );
    // The last spread is the one in the subscription.
    assert_diag_range_equals(
        d,
        &crate::support::range_for_token(&doc, text, "PostWithAuthor"),
    );
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_via_nested_spread_inside_fragment_body() {
    // The offending field is two fragments away from the subscription: the
    // spread fragment nests an object, and that object's selections come from
    // yet another fragment.
    let text = r#"
        fragment AuthorFields on User {
            id
            password
        }

        fragment PostWithAuthor on Post {
            id
            author {
                ...AuthorFields
            }
        }

        subscription OnPost {
            postAdded {
                ...PostWithAuthor
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "password".to_string(),
        ForbiddenFieldRule::new_operations(vec!["subscription".to_string()]),
    );

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::post_subscription_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Field 'password' is forbidden on type 'User' in subscription operations",
    );
    assert_diag_range_equals(
        d,
        &crate::support::range_for_token(&doc, text, "PostWithAuthor"),
    );
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_nested_inside_spread_fragment_ignored_at_the_spread() {
    let text = r#"
        fragment PostWithAuthor on Post {
            id
            author {
                id
                password
            }
        }

        subscription OnPost {
            postAdded {
                ...PostWithAuthor # graphox-ignore
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "password".to_string(),
        ForbiddenFieldRule::new_operations(vec!["subscription".to_string()]),
    );

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::post_subscription_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(1000)]
fn test_field_rules_terminate_on_a_fragment_cycle() {
    // Invalid GraphQL, but it reaches the LSP on every keystroke while a spread
    // is being written. The nested-selection walk descends through spreads and
    // builds a longer path each time, so it needs its own cycle guard: without
    // one this overflowed the stack instead of reporting the cycle.
    let text = r#"
        fragment PostFields on Post {
            id
            author {
                ...AuthorFields
            }
        }

        fragment AuthorFields on User {
            id
            password
            posts {
                ...PostFields
            }
        }

        query GetPosts {
            posts {
                ...PostFields
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "password".to_string(),
        ForbiddenFieldRule::new_operations(vec!["query".to_string()]),
    );
    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let schema = apollo_compiler::Schema::parse(
        r#"
            type Query { posts: [Post] }
            type Post { id: ID! author: User }
            type User { id: ID! password: String posts: [Post] }
        "#,
        "cycle_schema.graphql",
    )
    .unwrap()
    .validate()
    .unwrap();

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    assert!(
        diagnostics
            .iter()
            .any(|d| d.message.contains("Circular fragment reference")),
        "expected the cycle to be reported: {:#?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_nested_in_fragment_ignored_inside_the_fragment() {
    // Suppression written next to the offending selection, which is where the
    // field would be removed, holds for every operation spreading the fragment.
    let text = r#"
        fragment PostWithAuthor on Post {
            id
            author { # graphox-ignore
                id
                password
            }
        }

        subscription OnPost {
            postAdded {
                ...PostWithAuthor
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert(
        "password".to_string(),
        ForbiddenFieldRule::new_operations(vec!["subscription".to_string()]),
    );

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::post_subscription_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_forbidden_field_ignored_on_its_own_line() {
    // The diagnostic points at the field, so the comment works there too. This
    // is where the "Ignore forbidden field" code action writes it.
    let text = r#"
        query GetUsers {
            users {
                id
                password # graphox-ignore
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut forbidden_fields = AHashMap::default();
    forbidden_fields.insert("password".to_string(), ForbiddenFieldRule::new_always(true));

    let config = Config::default()
        .with_rules(RulesConfig::default().with_forbidden_fields(forbidden_fields));

    let diagnostics = doc.get_semantic_diagnostics(
        &fixtures::user_with_posts_schema()
            .clone()
            .validate()
            .unwrap(),
        &[],
        None,
        Some(&config),
        false,
        true,
    );

    assert_no_diagnostics(&diagnostics);
}
