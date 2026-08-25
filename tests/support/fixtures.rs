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
                    name: String
                    password: String
                }
                type Post {
                    id: ID!
                    title: String
                    author: User
                    secretField: String
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

static POST_SUBSCRIPTION_SCHEMA: OnceCell<Schema> = OnceCell::new();

/// Schema where a subscription and a query reach the same nested object, for
/// rules that depend on the operation type.
///
/// Contains:
/// - `type Query { posts: [Post] }`
/// - `type Subscription { postAdded: Post }`
/// - `type Post { id: ID! title: String author: User }`
/// - `type User { id: ID! name: String password: String }`
pub fn post_subscription_schema() -> &'static Schema {
    POST_SUBSCRIPTION_SCHEMA.get_or_init(|| {
        Schema::parse(
            r#"
                type Query {
                    posts: [Post]
                }
                type Subscription {
                    postAdded: Post
                }
                type Post {
                    id: ID!
                    title: String
                    author: User
                }
                type User {
                    id: ID!
                    name: String
                    password: String
                }
            "#,
            "post_subscription_schema.graphql",
        )
        .unwrap()
    })
}

static COLLIDING_RESPONSE_KEY_SCHEMA: OnceCell<Schema> = OnceCell::new();

/// Schema where the response key `subscription` is reachable via two different
/// paths that resolve to different object types — one that has an `id` field
/// and one that does not. Exercises required/forbidden-field checks that
/// resolve each `subscription` by its full response-key path rather than the
/// shared leaf name.
///
/// Contains:
/// - `type Query { soundZone: SoundZone account: Account }`
/// - `type SoundZone { id: ID! subscription: ZoneSubscription }`
/// - `type ZoneSubscription { state: String! }`  (no `id`)
/// - `type Account { id: ID! billing: Billing }`
/// - `type Billing { subscription: AccountSubscription }`
/// - `type AccountSubscription { id: ID! billingCycle: String! }`
pub fn colliding_response_key_schema() -> &'static Schema {
    COLLIDING_RESPONSE_KEY_SCHEMA.get_or_init(|| {
        Schema::parse(
            r#"
                type Query {
                    soundZone: SoundZone
                    account: Account
                }
                type SoundZone {
                    id: ID!
                    subscription: ZoneSubscription
                }
                type ZoneSubscription {
                    state: String!
                }
                type Account {
                    id: ID!
                    billing: Billing
                }
                type Billing {
                    subscription: AccountSubscription
                }
                type AccountSubscription {
                    id: ID!
                    billingCycle: String!
                }
            "#,
            "colliding_response_key_schema.graphql",
        )
        .unwrap()
    })
}

static ABSTRACT_SOURCE_SCHEMA: OnceCell<Schema> = OnceCell::new();

/// Schema where an object field resolves to a union and to an interface, each
/// of whose members nests a further object. Exercises field rules reaching
/// selections that sit behind a type condition partway down a path.
///
/// Contains:
/// - `type Query { zone: Zone }`, `type Subscription { zoneUpdate: Zone }`
/// - `type Zone { id: ID! permissions: [String!] source: PlayableSource item: Item }`
/// - `union PlayableSource = ScheduleSource | Manual`
/// - `type ScheduleSource { schedule: Schedule! }`
/// - `interface Item { id: ID! }`, `type SchedItem implements Item { schedule: Schedule! }`
/// - `type Schedule { id: ID! name: String permissions: [String!] }`
pub fn abstract_source_schema() -> &'static Schema {
    ABSTRACT_SOURCE_SCHEMA.get_or_init(|| {
        Schema::parse(
            r#"
                type Query {
                    zone: Zone
                }
                type Subscription {
                    zoneUpdate: Zone
                }
                type Zone {
                    id: ID!
                    permissions: [String!]
                    source: PlayableSource
                    item: Item
                }
                type Schedule {
                    id: ID!
                    name: String
                    permissions: [String!]
                }
                union PlayableSource = ScheduleSource | Manual
                type ScheduleSource {
                    schedule: Schedule!
                }
                type Manual {
                    id: ID!
                }
                interface Item {
                    id: ID!
                }
                type SchedItem implements Item {
                    id: ID!
                    schedule: Schedule!
                }
                type OtherItem implements Item {
                    id: ID!
                }
            "#,
            "abstract_source_schema.graphql",
        )
        .unwrap()
    })
}

static DEPRECATED_UNION_FIELD_SCHEMA: OnceCell<Schema> = OnceCell::new();

/// Schema where a deprecated field resolves to a union whose members each nest
/// an object carrying the same rule-relevant field. Exercises a
/// `# graphox-ignore` placed for the deprecation warning sitting directly above
/// selections the field rules still have to see.
///
/// Contains:
/// - `type Query { playback: Playback }`
/// - `type Playback { current: PlaybackItem }`
/// - `type PlaybackItem { id: ID! source: PlaybackSource @deprecated }`
/// - `union PlaybackSource = ScheduleSource | PlaylistSource`
/// - `type Schedule { id: ID! permissions: [String!] }` and the same on Playlist
pub fn deprecated_union_field_schema() -> &'static Schema {
    DEPRECATED_UNION_FIELD_SCHEMA.get_or_init(|| {
        Schema::parse(
            r#"
                type Query {
                    playback: Playback
                }
                type Playback {
                    current: PlaybackItem
                }
                type PlaybackItem {
                    id: ID!
                    source: PlaybackSource @deprecated(reason: "use sources")
                }
                union PlaybackSource = ScheduleSource | PlaylistSource
                type ScheduleSource {
                    schedule: Schedule!
                }
                type PlaylistSource {
                    playlist: Playlist!
                }
                type Schedule {
                    id: ID!
                    permissions: [String!]
                }
                type Playlist {
                    id: ID!
                    permissions: [String!]
                }
            "#,
            "deprecated_union_field_schema.graphql",
        )
        .unwrap()
    })
}

static PLAYABLE_SOURCE_SCHEMA: OnceCell<Schema> = OnceCell::new();

/// Schema shaped like a real union of playable sources: one member is a plain
/// object carrying the rule-relevant field directly, another reaches it through
/// a nested object. A fragment written on a member narrows the union by its own
/// type condition, with no `... on X` anywhere.
///
/// Contains:
/// - `type Query { playback: Playback }`, `type Subscription { playbackUpdate: Playback }`
/// - `type PlaybackItem { id: ID! source: PlayableSource @deprecated }`
/// - `union PlayableSource = ManuallyQueued | Playlist | ScheduleSource`
/// - `type Playlist { id: ID! name: String permissions: [String!] }`
/// - `type ScheduleSource { schedule: Schedule! }`
/// - `type Schedule { id: ID! name: String permissions: [String!] }`
pub fn playable_source_schema() -> &'static Schema {
    PLAYABLE_SOURCE_SCHEMA.get_or_init(|| {
        Schema::parse(
            r#"
                type Query {
                    playback: Playback
                }
                type Subscription {
                    playbackUpdate: Playback
                }
                type Playback {
                    id: ID!
                    current: PlaybackItem
                }
                type PlaybackItem {
                    id: ID!
                    source: PlayableSource @deprecated(reason: "use sources")
                }
                union PlayableSource = ManuallyQueued | Playlist | ScheduleSource
                type Playlist {
                    id: ID!
                    name: String
                    permissions: [String!]
                }
                type ScheduleSource {
                    schedule: Schedule!
                }
                type Schedule {
                    id: ID!
                    name: String
                    permissions: [String!]
                }
                type ManuallyQueued {
                    id: ID!
                }
            "#,
            "playable_source_schema.graphql",
        )
        .unwrap()
    })
}

static DISPLAYABLE_SCHEMA: OnceCell<Schema> = OnceCell::new();

/// Interface with several implementors, one of which recurses back into the
/// interface, reached through a field whose type is a *union* of those
/// implementors. A fragment on the interface spread at that field narrows the
/// union, and a fragment on a member spread inside it narrows again — the shape
/// where a single level of narrowing is not enough.
pub fn displayable_schema() -> &'static Schema {
    DISPLAYABLE_SCHEMA.get_or_init(|| {
        Schema::parse(
            r#"
                type Query {
                    edge: DisplayableEdge
                }
                type DisplayableEdge {
                    cursor: String
                    node: CardOrAlbum
                }
                union CardOrAlbum = EditorialCard | Album
                interface Displayable {
                    display: Display
                }
                type Display {
                    title: String
                }
                type EditorialCard implements Displayable {
                    id: ID!
                    description: String
                    display: Display
                    link: EditorialLink
                    item: Displayable
                }
                type EditorialLink implements Displayable {
                    id: ID!
                    display: Display
                }
                type Album implements Displayable {
                    id: ID!
                    display: Display
                }
            "#,
            "displayable_schema.graphql",
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

static SYNTAX_MATRIX_SCHEMA: OnceCell<Schema> = OnceCell::new();

/// Wide schema for exercising every syntactic route a selection can take to a
/// field. `secret` appears on several types at several depths, reachable
/// through plain objects, a union, two interfaces, an interface that
/// implements another, lists, and recursion.
pub fn syntax_matrix_schema() -> &'static Schema {
    SYNTAX_MATRIX_SCHEMA.get_or_init(|| {
        Schema::parse(
            r#"
                type Query {
                    zone: Zone
                    zones: [Zone!]
                }
                type Subscription {
                    zoneUpdate: Zone
                }
                type Zone {
                    id: ID!
                    secret: String
                    source: PlayableSource
                    sources: [PlayableSource!]
                    item: Item
                    node: Node
                    meta: Meta
                    child: Zone
                }
                type Meta {
                    id: ID!
                    secret: String
                    schedule: Schedule
                }
                union PlayableSource = ScheduleSource | Manual
                type ScheduleSource {
                    schedule: Schedule!
                    inner: Item
                    alt: PlayableSource
                }
                type Manual {
                    id: ID!
                    secret: String
                }
                interface Node {
                    id: ID!
                }
                interface Item implements Node {
                    id: ID!
                }
                type SchedItem implements Item & Node {
                    id: ID!
                    schedule: Schedule!
                }
                type OtherItem implements Item & Node {
                    id: ID!
                    secret: String
                }
                type Schedule {
                    id: ID!
                    name: String
                    secret: String
                    deep: Deep
                }
                type Deep {
                    id: ID!
                    secret: String
                }
            "#,
            "syntax_matrix_schema.graphql",
        )
        .unwrap()
    })
}
