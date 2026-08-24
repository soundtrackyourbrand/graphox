use ahash::AHashMap;
use apollo_compiler::Schema;
use graphox::Config;
use graphox::config::{RequiredFieldRule, RulesConfig};
use graphox::features::diagnostics::DocumentDiagnostics;
use tower_lsp_server::ls_types::*;

use crate::support::{
    assert_diag_range_equals, assert_diagnostic_severity, assert_diagnostic_with_message,
    assert_diagnostics_count, assert_no_diagnostics, create_doc, fixtures,
};

#[test]
#[ntest::timeout(300)]
fn test_required_field_always_true() {
    // Given: a query that selects the `users` field
    let text = r#"
        query GetUsers {
            users {
                id
                name
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Create config with required field rule (always true)
    let mut required_fields = AHashMap::default();
    required_fields.insert("users".to_string(), RequiredFieldRule::new_always(true));

    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

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

    // Expect no diagnostics
    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_missing_always_true() {
    let text = r#"
        query GetPosts {
            posts {
                id
                title
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Create config with required field rule (always true)
    let mut required_fields = AHashMap::default();
    required_fields.insert("users".to_string(), RequiredFieldRule::new_always(true));

    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

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
        "Required field 'users' must be selected in query operations",
    );
    assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
    assert_diag_range_equals(d, &crate::support::range_for_token(&doc, text, "posts"));
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_type_specific() {
    let text = r#"
        query GetData {
            users {
                id
            }
            posts {
                title
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let yaml = r#"
      required_fields:
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

    // Should error on User (missing name) but NOT on Post (name not required there)
    assert_diagnostics_count(&diagnostics, 1);
    assert_diagnostic_with_message(
        &diagnostics,
        "Required field 'name' must be selected in 'users'",
    );
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_type_override() {
    let text = r#"
        query GetUsers {
            users {
                name
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Global required, but disabled on User
    let yaml = r#"
      required_fields:
        id: true
        User:
          id: false
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
fn test_required_field_specific_operation_query() {
    let text = r#"
        query GetPosts {
            posts {
                id
                title
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Create config with required field rule (query operations only)
    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "users".to_string(),
        RequiredFieldRule::new_operations(vec!["query".to_string()]),
    );

    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

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
        "Required field 'users' must be selected in query operations",
    );
    assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
    assert_diag_range_equals(d, &crate::support::range_for_token(&doc, text, "posts"));
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_specific_operation_mutation_not_required() {
    let text = r#"
        mutation AddPost {
            addPost(title: "Hello") {
                id
                title
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Create config with required field rule (query operations only)
    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "users".to_string(),
        RequiredFieldRule::new_operations(vec!["query".to_string()]),
    );

    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

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
fn test_multiple_required_fields() {
    let text = r#"
        query GetPosts {
            posts {
                id
                title
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Create config with multiple required field rules
    let mut required_fields = AHashMap::default();
    required_fields.insert("users".to_string(), RequiredFieldRule::new_always(true));
    required_fields.insert("posts".to_string(), RequiredFieldRule::new_always(true));

    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

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

    // both users and posts should be selected on Query
    // but users is missing
    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Required field 'users' must be selected in query operations",
    );
    assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
    assert_diag_range_equals(d, &crate::support::range_for_token(&doc, text, "posts"));
}

#[test]
#[ntest::timeout(300)]
fn test_no_required_fields_config() {
    let text = r#"
        query GetPosts {
            posts {
                id
                title
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    // Empty config
    let config = Config::default();

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
fn test_required_field_with_reason() {
    let text = r#"
        query GetUsers {
            users {
                name
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "id".to_string(),
        RequiredFieldRule::new_always(true)
            .with_reason("IDs are required for client-side caching".to_string()),
    );

    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

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
        "Required field 'id' must be selected in 'users': IDs are required for client-side caching",
    );
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_case_sensitive() {
    let text = r#"
        query GetPosts {
            posts {
                id
                title
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut required_fields = AHashMap::default();
    required_fields.insert("USERS".to_string(), RequiredFieldRule::new_always(true));

    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

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

    // Rule for 'USERS' shouldn't match 'users' field name (case sensitive)
    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_not_in_schema() {
    let text = r#"
        query GetPosts {
            posts {
                id
                title
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "nonExistent".to_string(),
        RequiredFieldRule::new_always(true),
    );

    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

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
fn test_required_field_subscription() {
    let text = r#"
        subscription {
            postAdded {
                id
                title
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "id".to_string(),
        RequiredFieldRule::new_operations(vec!["subscription".to_string()]),
    );

    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

    let schema_text = r#"
        type Query { me: User }
        type Subscription { postAdded: Post }
        type User { id: ID! name: String }
        type Post { id: ID! title: String }
    "#;
    let schema = Schema::parse(schema_text, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    // postAdded selects id, so valid.
    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_ignored_with_inline_comment() {
    let schema = fixtures::user_with_posts_schema()
        .clone()
        .validate()
        .unwrap();

    let text = r#"
        query {
            users { # graphox-ignore
                name
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut required_fields = AHashMap::default();
    required_fields.insert("id".to_string(), RequiredFieldRule::new_always(true));

    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);
    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_inline_fragment() {
    let schema_content = r#"
        type Query {
            search: [SearchResult]
        }
        union SearchResult = User | Company
        type User {
            id: ID!
            name: String!
        }
        type Company {
            name: String!
        }
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        query {
            search {
                ... on User {
                    id
                }
                ... on Company {
                    name
                }
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut required_fields = AHashMap::default();
    required_fields.insert("id".to_string(), RequiredFieldRule::new_always(true));

    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    // Company doesn't have id field, so only User needs to select id (which it does)
    // No errors expected
    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_inline_fragment_partial_coverage() {
    let schema_content = r#"
        type Query {
            search: [SearchResult]
        }
        union SearchResult = User | Company
        type User {
            id: ID!
            name: String!
        }
        type Company {
            id: ID!
            name: String!
        }
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        query {
            search {
                ... on User {
                    id
                }
                ... on Company {
                    name
                }
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut required_fields = AHashMap::default();
    required_fields.insert("id".to_string(), RequiredFieldRule::new_always(true));

    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    // Company has id field but doesn't select it. User selects it.
    assert_diagnostics_count(&diagnostics, 1);
    assert_diagnostic_with_message(
        &diagnostics,
        "Required field 'id' must be selected in '... on Company'",
    );
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_nested_with_inline_fragment() {
    let schema_content = r#"
        type Query {
            users: [User]
        }
        type User {
            id: ID!
            name: String!
            profile: Profile
        }
        type Profile {
            id: ID!
            bio: String!
        }
    "#;
    let schema = Schema::parse(schema_content, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        query {
            users {
                name
                profile {
                    bio
                }
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut required_fields = AHashMap::default();
    required_fields.insert("id".to_string(), RequiredFieldRule::new_always(true));

    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    // id is required on User but not selected, and id is required on Profile but not selected
    assert_diagnostics_count(&diagnostics, 2);
    assert_diagnostic_with_message(
        &diagnostics,
        "Required field 'id' must be selected in 'users'",
    );
    assert_diagnostic_with_message(
        &diagnostics,
        "Required field 'id' must be selected in 'profile'",
    );
}

#[test]
#[ntest::timeout(500)]
fn test_required_id_with_fragment_spread() {
    let schema_text = r#"
        type Query {
            tracks(ids: [ID!]!): [Track]
        }
        type Track {
            id: ID!
            title: String!
            durationMs: Int!
            album: Album
            artists: [Artist]
        }
        type Album {
            id: ID!
            title: String!
            display: ProductCard
        }
        type Artist {
            id: ID!
            name: String!
        }
        type ProductCard {
            id: ID!
            url: String
        }
    "#;
    let schema = Schema::parse(schema_text, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let query_text = r#"
        fragment ProductCard on ProductCard {
            id
            url
        }

        fragment ResolvedTrack on Track {
            id
            title
            durationMs
            album {
                display {
                    ...ProductCard
                }
                id
                title
            }
            artists {
                id
                name
            }
        }

        query TrackQuery($trackIds: [ID!]!) {
            tracks(ids: $trackIds) {
                ...ResolvedTrack
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", query_text);

    let mut required_fields = AHashMap::default();
    required_fields.insert("id".to_string(), RequiredFieldRule::new_always(true));

    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    assert_no_diagnostics(&diagnostics);
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_required_fields_in_fragment_spread_with_inline_fragment() {
    let dir = tempfile::TempDir::new().expect("failed to create tempdir");
    let schema_text = r#"
        type Query {
            node: Node
        }
        interface Node {
            id: ID!
        }
        type User implements Node {
            id: ID!
            name: String!
        }
    "#;
    let schema_path = dir.path().join("schema.graphql");
    std::fs::write(&schema_path, schema_text).expect("write schema");

    let fragment_text = r#"
        fragment UserFields on Node {
            ... on User {
                name
            }
        }
    "#;
    let frag_uri = crate::support::write_project_file(&dir, "fragment.graphql", fragment_text);

    let query_text = r#"
        query GetNode {
            node {
                ...UserFields
            }
        }
    "#;
    let query_uri = crate::support::write_project_file(&dir, "query.graphql", query_text);

    let config = Config::new_test(
        dir.path().to_path_buf(),
        vec![
            graphox::config::ProjectConfig::default()
                .with_schema(graphox::config::SchemaSource::Single(
                    "schema.graphql".to_string(),
                ))
                .with_include(graphox::config::GlobPattern::Single(
                    "**/*.graphql".to_string(),
                ))
                .with_codegen(graphox::config::CodegenConfig::disabled()),
        ],
    )
    .with_rules(
        RulesConfig::default().with_required_fields(ahash::AHashMap::from([(
            "name".to_string(),
            RequiredFieldRule::new_always(true),
        )])),
    )
    .with_enable_schema_cache(false)
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    crate::support::lsp_did_open(&mut service, frag_uri.clone(), "graphql", 1, fragment_text).await;
    crate::support::lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let result = crate::support::lsp_request_diagnostics(&mut service, query_uri.clone()).await;
    let mut diagnostics = Vec::new();
    if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) =
        result
    {
        diagnostics = full_report.full_document_diagnostic_report.items;
    }

    assert!(
        diagnostics.is_empty(),
        "Should NOT report missing 'name' in fragment spread with inline fragment. Diagnostics: {:?}",
        diagnostics
    );
}

#[test]
fn test_required_field_partial_selection_with_alias() {
    let schema_text = "type Query { user: User } type User { id: ID! name: String }";
    let schema = Schema::parse(schema_text, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap();

    let text = r#"
        query {
            a: user { id }
            b: user { name }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut required_fields = AHashMap::default();
    required_fields.insert("id".to_string(), RequiredFieldRule::new_always(true));
    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

    let diagnostics = doc.get_semantic_diagnostics(&schema, &[], None, Some(&config), false, true);

    // 'a' has id, but 'b' doesn't.
    assert_diagnostics_count(&diagnostics, 1);
    assert_diagnostic_with_message(&diagnostics, "Required field 'id' must be selected in 'b'");
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_required_fields_merging_and_nesting_complex() {
    let dir = tempfile::TempDir::new().expect("failed to create tempdir");
    let schema_text = r#"
        type Query {
            playlist(id: ID!): Playlist
        }
        type Playlist {
            id: ID!
            permissions: [String!]
            composer: PlaylistComposer
        }
        union PlaylistComposer = SpotifyComposer | ExternalSpotifyComposer
        type SpotifyComposer {
            id: ID!
            syncedAt: String!
            permissions: [String!]
        }
        type ExternalSpotifyComposer {
            id: ID!
            spotifyPlaylistUri: String!
            permissions: [String!]
        }
    "#;
    let schema_path = dir.path().join("schema.graphql");
    std::fs::write(&schema_path, schema_text).expect("write schema");

    // Fragments in separate file
    let fragments_text = r#"
        fragment PlaylistFields on Playlist {
            id
            permissions
        }
        fragment ComposerPermissions on PlaylistComposer {
            ... on SpotifyComposer {
                spotifyComposerPermissions: permissions
            }
            ... on ExternalSpotifyComposer {
                externalSpotifyComposerId: id
                externalSpotifyComposerPermissions: permissions
            }
        }
    "#;
    let frags_uri = crate::support::write_project_file(&dir, "fragments.graphql", fragments_text);

    // Query that matches the reported bug structure exactly
    let query_text = r#"
        query SchedulePlaylistDoc($playlistId: ID!) {
            playlist(id: $playlistId) {
                ...PlaylistFields
                composer {
                    ... on SpotifyComposer {
                        id
                        syncedAt
                    }
                    ... on ExternalSpotifyComposer {
                        spotifyPlaylistUri
                    }
                    ...ComposerPermissions
                }
            }
        }
    "#;
    let query_uri = crate::support::write_project_file(&dir, "query.graphql", query_text);

    let config = Config::new_test(
        dir.path().to_path_buf(),
        vec![
            graphox::config::ProjectConfig::default()
                .with_schema(graphox::config::SchemaSource::Single(
                    "schema.graphql".to_string(),
                ))
                .with_include(graphox::config::GlobPattern::Single(
                    "**/*.graphql".to_string(),
                ))
                .with_codegen(graphox::config::CodegenConfig::disabled()),
        ],
    )
    .with_rules(
        RulesConfig::default().with_required_fields(ahash::AHashMap::from([
            ("id".to_string(), RequiredFieldRule::new_always(true)),
            (
                "permissions".to_string(),
                RequiredFieldRule::new_always(true),
            ),
            ("syncedAt".to_string(), RequiredFieldRule::new_always(true)),
        ])),
    )
    .with_enable_schema_cache(false)
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    crate::support::lsp_did_open(
        &mut service,
        frags_uri.clone(),
        "graphql",
        1,
        fragments_text,
    )
    .await;
    crate::support::lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let result = crate::support::lsp_request_diagnostics(&mut service, query_uri.clone()).await;
    let mut diagnostics = Vec::new();
    if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) =
        result
    {
        diagnostics = full_report.full_document_diagnostic_report.items;
    }

    assert!(
        diagnostics.is_empty(),
        "Complex merging and nesting should be valid. Diagnostics: {:?}",
        diagnostics
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_project_level_rules_override_global() {
    // Bug reproduction: Project-level `required_fields: false` should override global `required_fields`
    let dir = tempfile::TempDir::new().expect("failed to create tempdir");
    let schema_text = r#"
        type Query {
            users: [User]
            posts: [Post]
        }
        type User {
            id: ID!
            name: String!
        }
        type Post {
            id: ID!
            title: String!
        }
    "#;
    let schema_path = dir.path().join("schema.graphql");
    std::fs::write(&schema_path, schema_text).expect("write schema");

    // Query that doesn't select 'id' field - should fail with global rules but pass with project override
    let query_text = r#"
        query GetUsers {
            users {
                name
            }
        }
    "#;
    let query_uri = crate::support::write_project_file(&dir, "query.graphql", query_text);

    // Create config with:
    // 1. Global rules: required_fields { id: true } - should require 'id' field
    // 2. Project rules: required_fields: false - should override global
    let mut global_required_fields = ahash::AHashMap::default();
    global_required_fields.insert("id".to_string(), RequiredFieldRule::new_always(true));

    let config = Config::new_test(
        dir.path().to_path_buf(),
        vec![
            graphox::config::ProjectConfig::default()
                .with_schema(graphox::config::SchemaSource::Single(
                    "schema.graphql".to_string(),
                ))
                .with_include(graphox::config::GlobPattern::Single(
                    "**/*.graphql".to_string(),
                ))
                .with_rules(
                    RulesConfig::default().with_required_fields(ahash::AHashMap::default()),
                ),
        ],
    )
    .with_rules(RulesConfig::default().with_required_fields(global_required_fields))
    .with_enable_schema_cache(false)
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    crate::support::lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let result = crate::support::lsp_request_diagnostics(&mut service, query_uri.clone()).await;
    let mut diagnostics = Vec::new();
    if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) =
        result
    {
        diagnostics = full_report.full_document_diagnostic_report.items;
    }

    // The project has `required_fields: false` (empty hashmap), so it should override
    // the global `required_fields: { id: true }` and not report any missing 'id' errors
    assert!(
        diagnostics.is_empty(),
        "Project-level required_fields: false should override global required_fields. Diagnostics: {:?}",
        diagnostics
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_project_level_rules_without_override_uses_global() {
    // When project has no rules, it should use global rules
    let dir = tempfile::TempDir::new().expect("failed to create tempdir");
    let schema_text = r#"
        type Query {
            users: [User]
            posts: [Post]
        }
        type User {
            id: ID!
            name: String!
        }
        type Post {
            id: ID!
            title: String!
        }
    "#;
    let schema_path = dir.path().join("schema.graphql");
    std::fs::write(&schema_path, schema_text).expect("write schema");

    // Query that doesn't select 'id' field
    let query_text = r#"
        query GetUsers {
            users {
                name
            }
        }
    "#;
    let query_uri = crate::support::write_project_file(&dir, "query.graphql", query_text);

    // Create config with global rules but NO project-level rules override
    let mut global_required_fields = ahash::AHashMap::default();
    global_required_fields.insert("id".to_string(), RequiredFieldRule::new_always(true));

    let config = Config::new_test(
        dir.path().to_path_buf(),
        vec![
            graphox::config::ProjectConfig::default()
                .with_schema(graphox::config::SchemaSource::Single(
                    "schema.graphql".to_string(),
                ))
                .with_include(graphox::config::GlobPattern::Single(
                    "**/*.graphql".to_string(),
                )),
            // No .with_rules() - should use global
        ],
    )
    .with_rules(RulesConfig::default().with_required_fields(global_required_fields))
    .with_enable_schema_cache(false)
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    crate::support::lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let result = crate::support::lsp_request_diagnostics(&mut service, query_uri.clone()).await;
    let mut diagnostics = Vec::new();
    if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) =
        result
    {
        diagnostics = full_report.full_document_diagnostic_report.items;
    }

    // Without project-level override, global rules should apply and report missing 'id'
    assert_eq!(
        diagnostics.len(),
        1,
        "Should report missing 'id' field when using global rules without project override"
    );
    assert!(
        diagnostics[0].message.contains("Required field 'id'"),
        "Diagnostic should mention required field 'id': {:?}",
        diagnostics[0]
    );
}

#[tokio::test]
#[ntest::timeout(3000)]
async fn test_project_level_field_override_replaces_global() {
    // Test that project-level field overrides replace global rules entirely
    let dir = tempfile::TempDir::new().expect("failed to create tempdir");
    let schema_text = r#"
        type Query {
            users: [User]
        }
        type User {
            id: ID!
            permissions: [String]
            name: String!
        }
    "#;
    let schema_path = dir.path().join("schema.graphql");
    std::fs::write(&schema_path, schema_text).expect("write schema");

    // Query that selects ONLY 'name'
    let query_text = r#"
        query GetUsers {
            users {
                name
            }
        }
    "#;
    let query_uri = crate::support::write_project_file(&dir, "query.graphql", query_text);

    // Create config with:
    // 1. Global rules: required_fields { id: true, permissions: true }
    // 2. Project rules: required_fields: { permissions: false }
    // Result should be ONLY: { permissions: false }
    // 'id' should NOT be required anymore for this project.
    let mut global_required_fields = ahash::AHashMap::default();
    global_required_fields.insert("id".to_string(), RequiredFieldRule::new_always(true));
    global_required_fields.insert(
        "permissions".to_string(),
        RequiredFieldRule::new_always(true),
    );

    let mut project_required_fields = ahash::AHashMap::default();
    project_required_fields.insert(
        "permissions".to_string(),
        RequiredFieldRule::new_always(false),
    );

    let config = Config::new_test(
        dir.path().to_path_buf(),
        vec![
            graphox::config::ProjectConfig::default()
                .with_schema(graphox::config::SchemaSource::Single(
                    "schema.graphql".to_string(),
                ))
                .with_include(graphox::config::GlobPattern::Single(
                    "**/*.graphql".to_string(),
                ))
                .with_rules(RulesConfig::default().with_required_fields(project_required_fields)),
        ],
    )
    .with_rules(RulesConfig::default().with_required_fields(global_required_fields))
    .with_enable_schema_cache(false)
    .with_lsp_automatic_codegen(false);

    let (mut service, _handle) = crate::support::create_initialized_lsp_service(config).await;

    crate::support::lsp_did_open(&mut service, query_uri.clone(), "graphql", 1, query_text).await;

    let result = crate::support::lsp_request_diagnostics(&mut service, query_uri.clone()).await;
    let mut diagnostics = Vec::new();
    if let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full_report)) =
        result
    {
        diagnostics = full_report.full_document_diagnostic_report.items;
    }

    // Should be empty because:
    // - Project-level rules replaced global rules.
    // - Project-level rules only say 'permissions' is false (not required).
    // - 'id' is no longer required because it was only in global.
    assert!(
        diagnostics.is_empty(),
        "Should not report missing 'id' or 'permissions' because project rules replaced global rules. Diagnostics: {:?}",
        diagnostics
    );
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_same_response_key_different_types() {
    // The response key `subscription` is reached via two paths that resolve to
    // different types. `ZoneSubscription` has no `id` field, so `id` is not
    // required there; `AccountSubscription` has `id`, so it is required and
    // reported on the account subscription.
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

    let mut required_fields = AHashMap::default();
    required_fields.insert("id".to_string(), RequiredFieldRule::new_always(true));

    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

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

    // Exactly one diagnostic: `id` required on the AccountSubscription, anchored
    // at the `subscription` selection under billing, not the zone one.
    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Required field 'id' must be selected in 'subscription'",
    );
    assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
    assert_diag_range_equals(
        d,
        &crate::support::range_for_token_at_index(&doc, text, "subscription", 1),
    );
}

/// Shared schema for the fragment-definition rule checks below: mirrors the
/// `account.billing.subscription` shape from the original bug report.
fn account_schema() -> apollo_compiler::validation::Valid<Schema> {
    let schema_text = r#"
        type Query {
            account: Account
        }
        type Account {
            id: ID!
            businessName: String
            billing: Billing
            onboardingSoundZone: SoundZone
        }
        type Billing {
            id: ID!
            subscription: AccountSubscription
        }
        type AccountSubscription {
            id: ID!
            price: Int
        }
        type SoundZone {
            id: ID!
            name: String
        }
    "#;
    Schema::parse(schema_text, "schema.graphql")
        .unwrap()
        .validate()
        .unwrap()
}

fn required_id_config() -> Config {
    let mut required_fields = AHashMap::default();
    required_fields.insert("id".to_string(), RequiredFieldRule::new_always(true));
    Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields))
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_nested_inside_fragment_definition() {
    // A nested selection inside a fragment body is checked just like one inside
    // an operation, and the diagnostic lands on the fragment's own selection.
    let text = r#"
        fragment Checkout_Account on Account {
            id
            onboardingSoundZone {
                name
            }
        }

        query Checkout_account {
            account {
                ...Checkout_Account
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let diagnostics = doc.get_semantic_diagnostics(
        &account_schema(),
        &[],
        None,
        Some(&required_id_config()),
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Required field 'id' must be selected in 'onboardingSoundZone'",
    );
    assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
    assert_diag_range_equals(
        d,
        &crate::support::range_for_token(&doc, text, "onboardingSoundZone"),
    );
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_deeply_nested_inside_fragment_definition() {
    let text = r#"
        fragment AccountBilling on Account {
            id
            billing {
                id
                subscription {
                    price
                }
            }
        }

        query AccountSubscription {
            account {
                ...AccountBilling
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let diagnostics = doc.get_semantic_diagnostics(
        &account_schema(),
        &[],
        None,
        Some(&required_id_config()),
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Required field 'id' must be selected in 'subscription'",
    );
    assert_diag_range_equals(
        d,
        &crate::support::range_for_token(&doc, text, "subscription"),
    );
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_at_fragment_type_condition_left_to_the_operation() {
    // A fragment need not select the required field at its own type condition:
    // the consuming operation may select it next to the spread. Flagging the
    // fragment here would be a false positive, and the operation-side check
    // already covers the case where nobody selects it.
    let text = r#"
        fragment SubscriptionPrice on AccountSubscription {
            price
        }

        query AccountSubscription {
            account {
                id
                billing {
                    id
                    subscription {
                        id
                        ...SubscriptionPrice
                    }
                }
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let diagnostics = doc.get_semantic_diagnostics(
        &account_schema(),
        &[],
        None,
        Some(&required_id_config()),
        false,
        true,
    );

    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_missing_at_fragment_type_condition_reported_once() {
    // Nobody selects `id`, so the operation-side check reports it — once,
    // anchored at the spread site, not also inside the fragment.
    let text = r#"
        fragment SubscriptionPrice on AccountSubscription {
            price
        }

        query AccountSubscription {
            account {
                id
                billing {
                    id
                    subscription {
                        ...SubscriptionPrice
                    }
                }
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let diagnostics = doc.get_semantic_diagnostics(
        &account_schema(),
        &[],
        None,
        Some(&required_id_config()),
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Required field 'id' must be selected in 'subscription'",
    );
    assert_diag_range_equals(
        d,
        &crate::support::range_for_token(&doc, text, "subscription"),
    );
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_operation_scoped_rule_checked_at_the_spread() {
    // The enclosing operation is unknown inside a fragment, so a rule scoped to
    // specific operation types cannot be evaluated there. It is evaluated where
    // the fragment is spread instead, which is where the operation type is
    // known, and reported on the spread since the selection lives elsewhere.
    let text = r#"
        fragment AccountBilling on Account {
            billing {
                subscription {
                    price
                }
            }
        }

        query AccountSubscription {
            account {
                id
                ...AccountBilling
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "id".to_string(),
        RequiredFieldRule::new_operations(vec!["query".to_string()]),
    );
    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

    let diagnostics =
        doc.get_semantic_diagnostics(&account_schema(), &[], None, Some(&config), false, true);

    // Both objects nested in the fragment body are missing `id`.
    assert_diagnostics_count(&diagnostics, 2);
    let spread_range = crate::support::range_for_token(&doc, text, "AccountBilling");
    for expected in [
        "Required field 'id' must be selected in 'billing' inside fragment 'AccountBilling'",
        "Required field 'id' must be selected in 'subscription' inside fragment 'AccountBilling'",
    ] {
        let d = assert_diagnostic_with_message(&diagnostics, expected);
        assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
        assert_diag_range_equals(d, &spread_range);
    }
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_inside_fragment_satisfied_by_nested_spread() {
    // The nested selection gets `id` from a spread, not a literal field.
    let text = r#"
        fragment SubscriptionFields on AccountSubscription {
            id
            price
        }

        fragment AccountBilling on Account {
            id
            billing {
                id
                subscription {
                    ...SubscriptionFields
                }
            }
        }

        query AccountSubscription {
            account {
                ...AccountBilling
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let diagnostics = doc.get_semantic_diagnostics(
        &account_schema(),
        &[],
        None,
        Some(&required_id_config()),
        false,
        true,
    );

    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_inside_fragment_ignored_with_inline_comment() {
    let text = r#"
        fragment AccountBilling on Account {
            id
            billing {
                id
                subscription { # graphox-ignore
                    price
                }
            }
        }

        query AccountSubscription {
            account {
                ...AccountBilling
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let diagnostics = doc.get_semantic_diagnostics(
        &account_schema(),
        &[],
        None,
        Some(&required_id_config()),
        false,
        true,
    );

    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_fragment_state_does_not_leak_into_operation() {
    // One ValidationContext is reused across every definition in a document, so
    // a fragment's response-key bookkeeping must not bleed into a later
    // operation (or vice versa). Here only the fragment is at fault.
    let text = r#"
        query AccountSubscription {
            account {
                id
                billing {
                    id
                    subscription {
                        id
                        price
                    }
                }
            }
        }

        fragment AccountBilling on Account {
            id
            billing {
                id
                subscription {
                    price
                }
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let diagnostics = doc.get_semantic_diagnostics(
        &account_schema(),
        &[],
        None,
        Some(&required_id_config()),
        false,
        true,
    );

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Required field 'id' must be selected in 'subscription'",
    );
    // The fragment's `subscription`, not the operation's.
    assert!(
        d.range.start.line > 10,
        "expected the fragment's selection, got {:?}",
        d.range
    );
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_two_levels_deep_inside_spread_fragment() {
    // `subscription` sits two objects below the fragment's own type, so the
    // spread site only sees it if the whole path was recorded.
    let text = r#"
        fragment AccountBilling on Account {
            billing {
                id
                subscription {
                    price
                }
            }
        }

        query AccountSubscription {
            account {
                id
                ...AccountBilling
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "id".to_string(),
        RequiredFieldRule::new_operations(vec!["query".to_string()]),
    );
    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

    let diagnostics =
        doc.get_semantic_diagnostics(&account_schema(), &[], None, Some(&config), false, true);

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Required field 'id' must be selected in 'subscription' inside fragment 'AccountBilling'",
    );
    assert_diag_range_equals(
        d,
        &crate::support::range_for_token(&doc, text, "AccountBilling"),
    );
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_nested_spread_does_not_satisfy_the_parent() {
    // `id` is selected on Billing by the inner fragment. That must not count as
    // a selection on Account, which is what merging every spread in a
    // fragment's body into one response key used to do.
    let text = r#"
        fragment BillingFields on Billing {
            id
            subscription {
                id
                price
            }
        }

        fragment AccountFields on Account {
            businessName
            billing {
                ...BillingFields
            }
        }

        query AccountSubscription {
            account {
                ...AccountFields
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "id".to_string(),
        RequiredFieldRule::new_operations(vec!["query".to_string()]),
    );
    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

    let diagnostics =
        doc.get_semantic_diagnostics(&account_schema(), &[], None, Some(&config), false, true);

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Required field 'id' must be selected in 'account'",
    );
    assert_diagnostic_severity(d, DiagnosticSeverity::ERROR);
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_aliased_selection_inside_spread_fragment() {
    let text = r#"
        fragment AccountBilling on Account {
            primary: billing {
                subscription {
                    id
                    price
                }
            }
        }

        query AccountSubscription {
            account {
                id
                ...AccountBilling
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "id".to_string(),
        RequiredFieldRule::new_operations(vec!["query".to_string()]),
    );
    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

    let diagnostics =
        doc.get_semantic_diagnostics(&account_schema(), &[], None, Some(&config), false, true);

    // The alias is the response key, and the type behind it still resolves.
    assert_diagnostics_count(&diagnostics, 1);
    assert_diagnostic_with_message(
        &diagnostics,
        "Required field 'id' must be selected in 'primary' inside fragment 'AccountBilling'",
    );
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_satisfied_by_merging_spread_and_inline_selections() {
    // The operation and the fragment both select `subscription`, and between
    // them every required field is there. GraphQL merges the two into one
    // selection set, so the rule has to see them merged as well.
    let text = r#"
        fragment AccountBilling on Account {
            billing {
                subscription {
                    price
                }
            }
        }

        query AccountSubscription {
            account {
                id
                ...AccountBilling
                billing {
                    id
                    subscription {
                        id
                    }
                }
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "id".to_string(),
        RequiredFieldRule::new_operations(vec!["query".to_string()]),
    );
    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

    let diagnostics =
        doc.get_semantic_diagnostics(&account_schema(), &[], None, Some(&config), false, true);

    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_reported_once_for_a_path_both_sides_select() {
    // Same shape, but neither side selects `id` on the nested object. It is one
    // object, so it is one diagnostic, reported where the document selects it.
    let text = r#"
        fragment AccountBilling on Account {
            billing {
                subscription {
                    price
                }
            }
        }

        query AccountSubscription {
            account {
                id
                ...AccountBilling
                billing {
                    id
                    subscription {
                        price
                    }
                }
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "id".to_string(),
        RequiredFieldRule::new_operations(vec!["query".to_string()]),
    );
    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

    let diagnostics =
        doc.get_semantic_diagnostics(&account_schema(), &[], None, Some(&config), false, true);

    assert_diagnostics_count(&diagnostics, 1);
    let d = assert_diagnostic_with_message(
        &diagnostics,
        "Required field 'id' must be selected in 'subscription'",
    );
    // The document selects the path itself, so the fragment is not named and the
    // diagnostic stays on the inline selection.
    assert!(
        !d.message.contains("inside fragment"),
        "should not blame the fragment: {}",
        d.message
    );
    assert_diag_range_equals(
        d,
        &crate::support::range_for_token(&doc, text, "subscription"),
    );
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_nested_in_fragment_ignored_inside_the_fragment() {
    // The comment sits on the selection that would have to gain the field, so it
    // holds for every operation that spreads the fragment.
    let text = r#"
        fragment AccountBilling on Account {
            billing {
                id
                subscription { # graphox-ignore
                    price
                }
            }
        }

        query AccountSubscription {
            account {
                id
                ...AccountBilling
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "id".to_string(),
        RequiredFieldRule::new_operations(vec!["query".to_string()]),
    );
    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

    let diagnostics =
        doc.get_semantic_diagnostics(&account_schema(), &[], None, Some(&config), false, true);

    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_nested_in_fragment_ignored_at_the_spread() {
    let text = r#"
        fragment AccountBilling on Account {
            billing {
                subscription {
                    price
                }
            }
        }

        query AccountSubscription {
            account {
                id
                ...AccountBilling # graphox-ignore
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "id".to_string(),
        RequiredFieldRule::new_operations(vec!["query".to_string()]),
    );
    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

    let diagnostics =
        doc.get_semantic_diagnostics(&account_schema(), &[], None, Some(&config), false, true);

    assert_no_diagnostics(&diagnostics);
}

#[test]
#[ntest::timeout(300)]
fn test_required_field_ignore_inside_fragment_is_per_selection() {
    // Only the selection carrying the comment is exempt; its sibling path is
    // still reported, so an ignore cannot quietly cover a whole subtree.
    let text = r#"
        fragment AccountBilling on Account {
            onboardingSoundZone { # graphox-ignore
                name
            }
            billing {
                subscription {
                    price
                }
            }
        }

        query AccountSubscription {
            account {
                id
                ...AccountBilling
            }
        }
    "#;
    let doc = create_doc("file:///test.graphql", text);

    let mut required_fields = AHashMap::default();
    required_fields.insert(
        "id".to_string(),
        RequiredFieldRule::new_operations(vec!["query".to_string()]),
    );
    let config =
        Config::default().with_rules(RulesConfig::default().with_required_fields(required_fields));

    let diagnostics =
        doc.get_semantic_diagnostics(&account_schema(), &[], None, Some(&config), false, true);

    assert_diagnostics_count(&diagnostics, 2);
    assert_diagnostic_with_message(
        &diagnostics,
        "Required field 'id' must be selected in 'billing' inside fragment 'AccountBilling'",
    );
    assert_diagnostic_with_message(
        &diagnostics,
        "Required field 'id' must be selected in 'subscription' inside fragment 'AccountBilling'",
    );
}
