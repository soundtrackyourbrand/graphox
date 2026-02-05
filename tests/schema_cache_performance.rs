// Test to verify that the two-tier schema cache is working correctly

use graphql_rust::{config::SchemaSource, schema_cache};
use std::time::Instant;
use tempfile::tempdir;

#[test]
#[ntest::timeout(5000)]
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
    schema_cache::clear_cache().unwrap();

    // First load - should be slow (no cache)
    let start = Instant::now();
    let schema1 = graphql_rust::schema::load_and_validate_schema(dir.path(), &source).unwrap();
    let first_load_time = start.elapsed();

    // Second load - should be MUCH faster (memory cache hit)
    let start = Instant::now();
    let schema2 = graphql_rust::schema::load_and_validate_schema(dir.path(), &source).unwrap();
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
    let _schema3 = graphql_rust::schema::load_and_validate_schema(dir.path(), &source).unwrap();
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
    schema_cache::clear_cache().unwrap();
}

#[test]
#[ntest::timeout(5000)]
fn test_cache_invalidation_on_file_change() {
    let dir = tempdir().unwrap();
    let schema_path = dir.path().join("schema.graphql");

    // Create initial schema
    std::fs::write(&schema_path, "type Query { hello: String }").unwrap();
    let source = SchemaSource::Single("schema.graphql".to_string());

    schema_cache::clear_cache().unwrap();

    // Load and cache
    let schema1 = graphql_rust::schema::load_and_validate_schema(dir.path(), &source).unwrap();

    // Sleep to ensure mtime changes
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Modify the schema
    std::fs::write(&schema_path, "type Query { world: String }").unwrap();

    // Load again - should invalidate cache and return new schema
    let schema2 = graphql_rust::schema::load_and_validate_schema(dir.path(), &source).unwrap();

    // They should NOT be the same Arc (different schemas)
    assert!(
        !std::sync::Arc::ptr_eq(&schema1, &schema2),
        "Cache should be invalidated when file changes"
    );

    schema_cache::clear_cache().unwrap();
}
