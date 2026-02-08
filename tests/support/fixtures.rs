//! Common test fixtures for GraphQL schemas and queries.
//!
//! All fixtures are cached using OnceCell for performance.
//!
//! # Usage
//!
//! ```rust
//! use tests::fixtures::{self, user_schema};
//!
//! let schema = user_schema().clone().validate().unwrap();
//! ```

use apollo_compiler::Schema;
use once_cell::sync::OnceCell;

// =============================================================================
// Schemas
// =============================================================================

static USER_SCHEMA: OnceCell<Schema> = OnceCell::new();

/// Schema with a simple User type and Query.user field
///
/// Contains:
/// - `type Query { user: User }`
/// - `type User { id: ID! name: String }`
pub fn user_schema() -> &'static Schema {
    USER_SCHEMA.get_or_init(|| {
        Schema::parse(
            r#"
                type Query { user: User }
                type User { id: ID! name: String }
            "#,
            "user_schema.graphql",
        )
        .unwrap()
    })
}

static USER_WITH_DEPRECATED_SCHEMA: OnceCell<Schema> = OnceCell::new();

/// Schema with User containing a deprecated field
///
/// Contains:
/// - `type Query { user: User }`
/// - `type User { id: ID! name: String oldField: String @deprecated(...) }`
pub fn user_with_deprecated_field_schema() -> &'static Schema {
    USER_WITH_DEPRECATED_SCHEMA.get_or_init(|| {
        Schema::parse(
            r#"
                type Query { user: User }
                type User {
                    id: ID!
                    name: String
                    oldField: String @deprecated(reason: "Use username instead")
                }
            "#,
            "user_deprecated_schema.graphql",
        )
        .unwrap()
    })
}

static PLAYLIST_SCHEMA: OnceCell<Schema> = OnceCell::new();

/// Schema for playlist testing with nested types
///
/// Contains:
/// - `type Query { playlist(id: ID!): Playlist }`
/// - `type Playlist { id: ID! permissions: [String] }`
pub fn playlist_schema() -> &'static Schema {
    PLAYLIST_SCHEMA.get_or_init(|| {
        Schema::parse(
            r#"
                type Query { playlist(id: ID!): Playlist }
                type Playlist {
                    id: ID!
                    permissions: [String]
                }
            "#,
            "playlist_schema.graphql",
        )
        .unwrap()
    })
}

static INPUT_WITH_DEPRECATED_SCHEMA: OnceCell<Schema> = OnceCell::new();

/// Schema with input type containing deprecated field
///
/// Contains:
/// - `input CreateUserInput { username: String! oldField: String @deprecated(...) newField: String }`
/// - `type Query { test(input: CreateUserInput): String }`
pub fn input_with_deprecated_field_schema() -> &'static Schema {
    INPUT_WITH_DEPRECATED_SCHEMA.get_or_init(|| {
        Schema::parse(
            r#"
                input CreateUserInput {
                    username: String!
                    oldField: String @deprecated(reason: "Use newField")
                    newField: String
                }
                type Query { test(input: CreateUserInput): String }
            "#,
            "input_deprecated_schema.graphql",
        )
        .unwrap()
    })
}

static UNION_INTERFACE_SCHEMA: OnceCell<Schema> = OnceCell::new();

/// Schema demonstrating unions and interfaces
///
/// Contains:
/// - `interface Named { name: String! }`
/// - `type User implements Named { id: ID! name: String! }`
/// - `type Bot implements Named { id: ID! name: String! version: String! }`
/// - `union SearchResult = User | Bot`
/// - `type Query { search(term: String!): [SearchResult] }`
pub fn union_interface_schema() -> &'static Schema {
    UNION_INTERFACE_SCHEMA.get_or_init(|| {
        Schema::parse(
            r#"
                interface Named {
                    name: String!
                }
                type User implements Named {
                    id: ID!
                    name: String!
                }
                type Bot implements Named {
                    id: ID!
                    name: String!
                    version: String!
                }
                union SearchResult = User | Bot
                type Query {
                    search(term: String!): [SearchResult]
                }
            "#,
            "union_interface_schema.graphql",
        )
        .unwrap()
    })
}

static USER_WITH_POSTS_SCHEMA: OnceCell<Schema> = OnceCell::new();

/// Schema with User and Posts for testing required fields
///
/// Contains:
/// - `type Query { users: [User] posts: [Post] }`
/// - `type User { id: ID! username: String }`
/// - `type Post { id: ID! title: String }`
pub fn user_with_posts_schema() -> &'static Schema {
    USER_WITH_POSTS_SCHEMA.get_or_init(|| {
        Schema::parse(
            r#"
                type Query {
                    users: [User]
                    posts: [Post]
                }
                type User {
                    id: ID!
                    username: String
                }
                type Post {
                    id: ID!
                    title: String
                    author: User
                }
            "#,
            "user_with_posts_schema.graphql",
        )
        .unwrap()
    })
}

static USER_SUBSCRIPTION_SCHEMA: OnceCell<Schema> = OnceCell::new();

/// Schema with subscription support for testing
///
/// Contains:
/// - `type Query { users: [User] }`
/// - `type Mutation { createUser(username: String!): User }`
/// - `type Subscription { userAdded: User }`
/// - `type User { id: ID! username: String! }`
pub fn user_subscription_schema() -> &'static Schema {
    USER_SUBSCRIPTION_SCHEMA.get_or_init(|| {
        Schema::parse(
            r#"
                type Query {
                    users: [User]
                }
                type Mutation {
                    createUser(username: String!): User
                }
                type Subscription {
                    userAdded: User
                }
                type User {
                    id: ID!
                    username: String!
                }
            "#,
            "user_subscription_schema.graphql",
        )
        .unwrap()
    })
}

// =============================================================================
// Query Strings
// =============================================================================

/// Query that retrieves a user with id and name
pub fn get_user_query() -> &'static str {
    r#"
        query GetUser {
            user {
                id
                name
            }
        }
    "#
}

/// Query with a missing field (for validation testing)
pub fn query_with_missing_field() -> &'static str {
    r#"
        query GetUser {
            user {
                id
                nonExistentField
            }
        }
    "#
}

/// Query with users (for required fields testing)
pub fn query_with_users() -> &'static str {
    r#"
        query GetUsers {
            users {
                id
                username
            }
        }
    "#
}

/// Query with posts (for required fields testing)
pub fn query_with_posts() -> &'static str {
    r#"
        query GetPosts {
            posts {
                id
                title
            }
        }
    "#
}

/// Query using a fragment spread
pub fn query_with_fragment_spread(fragment_name: &str) -> String {
    format!(
        r#"
            query {{
                user {{
                    ...{0}
                }}
            }}
        "#,
        fragment_name
    )
}

/// Fragment definition for User fields
pub fn user_fragment(fragment_name: &str) -> String {
    format!(
        r#"
            fragment {0} on User {{
                id
            }}
        "#,
        fragment_name
    )
}

/// Fragment with a field that doesn't exist
pub fn fragment_with_missing_field(fragment_name: &str) -> String {
    format!(
        r#"
            fragment {0} on User {{
                id
                missingField
            }}
        "#,
        fragment_name
    )
}

/// Fragment with inline field that doesn't exist
pub fn inline_fragment_with_missing_field() -> &'static str {
    r#"
        query {
            user {
                ... on User {
                    id
                    nonExistentOnUser
                }
            }
        }
    "#
}

/// Circular fragment definition A -> B -> A
pub fn circular_fragment_a() -> &'static str {
    r#"
        fragment FragA on User { ...FragB }
        fragment FragB on User { ...FragA }
    "#
}

/// Three-way circular fragment A -> B -> C -> A
pub fn three_way_circular_fragments() -> &'static str {
    r#"
        fragment A on User { ...B }
        fragment B on User { ...C }
        fragment C on User { ...A }
    "#
}

/// Query with aliased field
pub fn query_with_alias() -> &'static str {
    r#"
        query GetUsers {
            user {
                u: name
                email
            }
        }
    "#
}

/// Query using block strings and comments
pub fn query_with_block_string_and_comment() -> &'static str {
    r#"
        query GetUser($id: ID! = """123""") # This is a comment
        {
            user(id: $id) {
                id
            }
        }
    "#
}

/// Query with input using deprecated field
pub fn query_with_deprecated_input_field() -> &'static str {
    r#"
        query Test {
            test(input: { username: "test", oldField: "value" })
        }
    "#
}

/// Valid union query with inline fragments
pub fn valid_union_query() -> &'static str {
    r#"
        query Search($term: String!) {
            search(term: $term) {
                ... on User { id name }
                ... on Bot { id name version }
            }
        }
    "#
}

/// Invalid union query - field not on union
pub fn invalid_union_query() -> &'static str {
    r#"
        query {
            search(term: "foo") {
                id
            }
        }
    "#
}

/// Valid interface query
pub fn valid_interface_query() -> &'static str {
    r#"
        query {
            search(term: "foo") {
                ... on Named { name }
            }
        }
    "#
}
