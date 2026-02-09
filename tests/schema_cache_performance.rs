// Test to verify that the two-tier schema cache is working correctly

use apollo_compiler::schema::ExtendedType;
use graphql_rust::schema;
use graphql_rust::{config::SchemaSource, schema_cache};
use std::time::Instant;
use tempfile::tempdir;

#[test]
#[ntest::timeout(500)]
fn test_memory_cache_performance() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");

    // Create a moderately-sized schema
    let schema_content = r#"
        type Query {
            user(id: ID!): User
            users: [User!]!
            post(id: ID!): Post
            posts: [Post!]!
        }
        
        type User {
            id: ID!
            name: String!
            email: String!
            posts: [Post!]!
            comments: [Comment!]!
        }
        
        type Post {
            id: ID!
            title: String!
            content: String!
            author: User!
            comments: [Comment!]!
            tags: [String!]!
        }
        
        type Comment {
            id: ID!
            text: String!
            author: User!
            post: Post!
        }
        
        type Mutation {
            createUser(name: String!, email: String!): User!
            createPost(title: String!, content: String!, authorId: ID!): Post!
            createComment(text: String!, authorId: ID!, postId: ID!): Comment!
        }
    "#;

    std::fs::write(&schema_path, schema_content).unwrap();

    let source = SchemaSource::Single("schema.graphql".to_string());

    // Clear both caches
    let _ = schema_cache::clear_cache();

    // First load - should be slow (no cache)
    let start = Instant::now();
    let schema1 = schema::load_and_validate_schema(dir.path(), &source).unwrap();
    let first_load_time = start.elapsed();

    // Second load - should be MUCH faster (memory cache hit)
    let start = Instant::now();
    let schema2 = schema::load_and_validate_schema(dir.path(), &source).unwrap();
    let second_load_time = start.elapsed();

    // Verify they're the same Arc (pointer equality)
    assert!(
        std::sync::Arc::ptr_eq(&schema1, &schema2),
        "Memory cache should return the same Arc instance"
    );

    println!("First load (no cache):  {:?}", first_load_time);
    println!("Second load (mem cache): {:?}", second_load_time);
    println!(
        "Speedup: {:.1}x faster",
        first_load_time.as_micros() as f64 / second_load_time.as_micros() as f64
    );

    // Memory cache should be at least 10x faster (typically 100-1000x)
    assert!(
        second_load_time < first_load_time / 10,
        "Memory cache should be at least 10x faster. First: {:?}, Second: {:?}",
        first_load_time,
        second_load_time
    );

    // Clear memory cache only
    schema_cache::clear_memory_cache();

    // Third load - should use disk cache (slower than memory, faster than first load)
    let start = Instant::now();
    let _schema3 = schema::load_and_validate_schema(dir.path(), &source).unwrap();
    let third_load_time = start.elapsed();

    println!("Third load (disk cache): {:?}", third_load_time);

    // Disk cache should be faster than no cache, but slower than memory cache
    assert!(
        third_load_time < first_load_time,
        "Disk cache should be faster than no cache. Disk: {:?}, None: {:?}",
        third_load_time,
        first_load_time
    );

    // Clean up
    let _ = schema_cache::clear_cache();
}

#[test]
#[ntest::timeout(500)]
fn test_cache_invalidation_on_file_change() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");

    // Create initial schema
    std::fs::write(&schema_path, "type Query { hello: String }").unwrap();
    let source = SchemaSource::Single("schema.graphql".to_string());

    let _ = schema_cache::clear_cache();

    // Load and cache
    let schema1 = schema::load_and_validate_schema(dir.path(), &source).unwrap();

    // Sleep to ensure mtime changes
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Modify the schema
    std::fs::write(&schema_path, "type Query { world: String }").unwrap();

    // Load again - should invalidate cache and return new schema
    let schema2 = schema::load_and_validate_schema(dir.path(), &source).unwrap();

    // They should NOT be the same Arc (different schemas)
    assert!(
        !std::sync::Arc::ptr_eq(&schema1, &schema2),
        "Cache should be invalidated when file changes"
    );

    let _ = schema_cache::clear_cache();
}

#[test]
#[ntest::timeout(500)]
fn test_cache_corruption_handling() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");

    std::fs::write(&schema_path, "type Query { hello: String }").unwrap();
    let source = SchemaSource::Single("schema.graphql".to_string());

    let _ = schema_cache::clear_cache();

    let schema1 = schema::load_and_validate_schema(dir.path(), &source).unwrap();
    let query1 = schema1.types.get("Query");
    assert_eq!(
        query1
            .map(|t| {
                match t {
                    ExtendedType::Object(obj) => obj.fields.len(),
                    ExtendedType::Interface(iface) => iface.fields.len(),
                    _ => 0,
                }
            })
            .unwrap_or(0),
        1
    );

    std::thread::sleep(std::time::Duration::from_millis(10));

    std::fs::write(&schema_path, "type Query { hello: String world: String }").unwrap();

    let schema2 = schema::load_and_validate_schema(dir.path(), &source).unwrap();
    let query2 = schema2.types.get("Query");
    assert_eq!(
        query2
            .map(|t| {
                match t {
                    ExtendedType::Object(obj) => obj.fields.len(),
                    ExtendedType::Interface(iface) => iface.fields.len(),
                    _ => 0,
                }
            })
            .unwrap_or(0),
        2
    );

    assert!(
        !std::sync::Arc::ptr_eq(&schema1, &schema2),
        "Cache should be invalidated and return new schema"
    );

    let _ = schema_cache::clear_cache();
}

#[test]
#[ntest::timeout(600)]
fn test_cache_memory_pressure() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");

    let large_schema = (0..500)
        .map(|i| format!("type Item{} {{ id: ID! name{}: String! }}", i, i))
        .chain(std::iter::once("type Query {".to_string()))
        .chain((0..500).map(|i| format!("    item{}: Item{}", i, i)))
        .chain(std::iter::once("}".to_string()))
        .collect::<Vec<_>>()
        .join("\n");

    std::fs::write(&schema_path, &large_schema).unwrap();
    let source = SchemaSource::Single("schema.graphql".to_string());

    let _ = schema_cache::clear_cache();

    let start = std::time::Instant::now();
    let schema1 = schema::load_and_validate_schema(dir.path(), &source).unwrap();
    let first_load = start.elapsed();

    let start = std::time::Instant::now();
    let schema2 = schema::load_and_validate_schema(dir.path(), &source).unwrap();
    let cached_load = start.elapsed();

    println!("Large schema first load: {:?}", first_load);
    println!("Large schema cached load: {:?}", cached_load);

    assert!(
        cached_load < first_load / 5,
        "Cached load should be significantly faster. First: {:?}, Cached: {:?}",
        first_load,
        cached_load
    );

    assert!(
        std::sync::Arc::ptr_eq(&schema1, &schema2),
        "Cached schema should return same Arc"
    );

    let _ = schema_cache::clear_cache();
}
