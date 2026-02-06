//! Two-tier cache for parsed GraphQL schemas
//!
//! This module provides both in-memory and disk-based caching for schemas to avoid
//! expensive re-parsing during benchmark runs and other repeated operations.
//!
//! ## Two-tier caching strategy
//!
//! 1. **Memory cache** (L1): Holds fully parsed and validated `Schema` objects
//!    - Fastest: No I/O, no parsing, no validation
//!    - Lifetime: Process duration
//!    - Invalidation: Checks file mtimes before returning
//!
//! 2. **Disk cache** (L2): Holds merged schema text
//!    - Fast: Skips file I/O and merging, but still needs parsing/validation
//!    - Lifetime: Persistent across runs
//!    - Invalidation: Automatic via file mtime checks
//!
//! ## How it works
//!
//! When loading a schema:
//! 1. Check memory cache → return if valid (saves ~60-90ms)
//! 2. Check disk cache → parse and cache in memory if valid (saves ~5-10ms)
//! 3. Load from files → parse, validate, and cache both tiers
//!
//! ## Cache location (disk only)
//!
//! - macOS: `~/Library/Caches/graphql-rust/`
//! - Linux: `~/.cache/graphql-rust/`
//! - Windows: `%LOCALAPPDATA%\graphql-rust\cache\`
//! - Custom: Set `GRAPHQL_CACHE_DIR` environment variable
//!
//! ## Performance impact
//!
//! Memory cache (L1): 95-99% faster than parsing
//! Disk cache (L2): 10-30% faster for small schemas, 50-80% for large schemas
//!
//! ## Configuration
//!
//! Caching is enabled by default. To disable it, set `enable_schema_cache: false` in
//! your `graphql.yaml` configuration file.

use crate::config::SchemaSource;
use apollo_compiler::{Schema, validation::Valid};
use dashmap::DashMap;
use fnv::FnvHashMap;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// Metadata for cache validation
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheMetadata {
    /// Maps file path to its last modification time
    file_mtimes: FnvHashMap<String, SystemTime>,
}

/// Cache entry containing the schema text and metadata (disk cache)
#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    /// Merged schema text ready for parsing
    merged_schema: String,
    /// Metadata for validation
    metadata: CacheMetadata,
}

/// In-memory cache entry for fully parsed schemas
#[derive(Debug, Clone)]
struct MemoryCacheEntry {
    /// Parsed and validated schema
    schema: Arc<Valid<Schema>>,
    /// Metadata for validation
    metadata: CacheMetadata,
}

/// Global in-memory cache for parsed schemas
/// Uses DashMap for thread-safe concurrent access
static MEMORY_CACHE: Lazy<DashMap<String, MemoryCacheEntry>> = Lazy::new(DashMap::new);

impl CacheMetadata {
    /// Create metadata from a list of file paths
    fn from_files(base_dir: &Path, files: &[String]) -> Result<Self, String> {
        let mut file_mtimes = FnvHashMap::default();

        for file in files {
            let path = base_dir.join(file);
            match fs::metadata(&path) {
                Ok(meta) => match meta.modified() {
                    Ok(mtime) => {
                        file_mtimes.insert(file.clone(), mtime);
                    }
                    Err(e) => {
                        return Err(format!("Failed to get mtime for {}: {}", path.display(), e));
                    }
                },
                Err(e) => {
                    return Err(format!(
                        "Failed to read metadata for {}: {}",
                        path.display(),
                        e
                    ));
                }
            }
        }

        Ok(Self { file_mtimes })
    }

    /// Check if this metadata is still valid (no files have changed)
    fn is_valid(&self, base_dir: &Path) -> bool {
        for (file, cached_mtime) in &self.file_mtimes {
            let path = base_dir.join(file);
            match fs::metadata(&path) {
                Ok(meta) => match meta.modified() {
                    Ok(mtime) => {
                        if &mtime != cached_mtime {
                            return false;
                        }
                    }
                    Err(_) => return false,
                },
                Err(_) => return false,
            }
        }
        true
    }
}

// ============================================================================
// Memory Cache (L1) - Fully parsed schemas
// ============================================================================

/// Try to load a fully parsed and validated schema from memory cache
///
/// Returns `Some(Arc<Valid<Schema>>)` if cached and still valid, `None` otherwise
pub fn try_load_parsed_from_memory(
    base_dir: &Path,
    source: &SchemaSource,
) -> Option<Arc<Valid<Schema>>> {
    let key = source.as_key();

    // Get from cache
    let entry = MEMORY_CACHE.get(&key)?;

    // Validate that files haven't changed
    if !entry.metadata.is_valid(base_dir) {
        // Cache is stale, remove it
        drop(entry);
        MEMORY_CACHE.remove(&key);
        return None;
    }

    #[cfg(debug_assertions)]
    eprintln!("✓ Memory cache hit for {}", key);

    Some(entry.schema.clone())
}

/// Save a parsed and validated schema to memory cache
pub fn save_parsed_to_memory(
    base_dir: &Path,
    source: &SchemaSource,
    schema: Arc<Valid<Schema>>,
) -> Result<(), String> {
    let key = source.as_key();
    let files = source.files();
    let metadata = CacheMetadata::from_files(base_dir, &files)?;

    let entry = MemoryCacheEntry { schema, metadata };

    MEMORY_CACHE.insert(key, entry);

    Ok(())
}

/// Clear a specific schema from memory cache
pub fn clear_memory_cache_for(source: &SchemaSource) {
    let key = source.as_key();
    MEMORY_CACHE.remove(&key);
}

/// Clear all schemas from memory cache
pub fn clear_memory_cache() {
    MEMORY_CACHE.clear();
}

// ============================================================================
// Disk Cache (L2) - Merged schema text
// ============================================================================

/// Get the cache directory path
fn get_cache_dir() -> PathBuf {
    let cache_dir = if let Ok(dir) = std::env::var("GRAPHQL_CACHE_DIR") {
        PathBuf::from(dir)
    } else if let Some(cache_home) = dirs::cache_dir() {
        cache_home.join("graphql-rust")
    } else {
        PathBuf::from(".graphql-cache")
    };
    cache_dir
}

/// Get the cache file path for a given schema source
fn get_cache_path(source: &SchemaSource) -> PathBuf {
    let cache_dir = get_cache_dir();

    // Create a stable filename from the schema key
    let key = source.as_key();
    let hash = {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    };

    cache_dir.join(format!("schema-{:x}.cache", hash))
}

/// Try to load a schema from cache
///
/// Returns `Some(merged_schema_text)` if the cache is valid, `None` otherwise
pub fn try_load_from_cache(base_dir: &Path, source: &SchemaSource) -> Option<String> {
    let cache_path = get_cache_path(source);

    if !cache_path.exists() {
        return None;
    }

    // Read and deserialize cache entry
    let cache_data = fs::read(&cache_path).ok()?;
    let entry: CacheEntry = bincode::deserialize(&cache_data).ok()?;

    // Validate metadata
    if !entry.metadata.is_valid(base_dir) {
        // Cache is stale, remove it
        let _ = fs::remove_file(&cache_path);
        return None;
    }

    Some(entry.merged_schema)
}

/// Save a merged schema to cache
pub fn save_to_cache(
    base_dir: &Path,
    source: &SchemaSource,
    merged_schema: &str,
) -> Result<(), String> {
    let files = source.files();
    let metadata = CacheMetadata::from_files(base_dir, &files)?;

    let entry = CacheEntry {
        merged_schema: merged_schema.to_string(),
        metadata,
    };

    // Serialize the entry
    let cache_data = bincode::serialize(&entry)
        .map_err(|e| format!("Failed to serialize cache entry: {}", e))?;

    // Ensure cache directory exists
    let cache_path = get_cache_path(source);
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create cache directory: {}", e))?;
    }

    // Write to cache file
    fs::write(&cache_path, cache_data).map_err(|e| format!("Failed to write cache file: {}", e))?;

    Ok(())
}

/// Clear all cached schemas (both memory and disk)
pub fn clear_cache() -> Result<(), String> {
    // Clear memory cache
    clear_memory_cache();

    // Clear disk cache
    let cache_dir = get_cache_dir();

    if cache_dir.exists() {
        fs::remove_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to clear cache directory: {}", e))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    #[ntest::timeout(1000)]
    fn test_cache_metadata_validation() {
        let dir = tempdir().unwrap();
        let schema_path = dir.path().join("schema.graphql");

        // Create a schema file
        let mut file = fs::File::create(&schema_path).unwrap();
        writeln!(file, "type Query {{ hello: String }}").unwrap();
        drop(file);

        // Create metadata
        let files = vec!["schema.graphql".to_string()];
        let metadata = CacheMetadata::from_files(dir.path(), &files).unwrap();

        // Should be valid immediately
        assert!(metadata.is_valid(dir.path()));

        // Sleep to ensure mtime will be different
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Modify the file
        let mut file = fs::OpenOptions::new()
            .write(true)
            .append(true)
            .open(&schema_path)
            .unwrap();
        writeln!(file, "type Mutation {{ update: Boolean }}").unwrap();
        drop(file);

        // Should now be invalid
        assert!(!metadata.is_valid(dir.path()));
    }

    #[test]
    #[ntest::timeout(1000)]
    fn test_cache_round_trip() {
        let dir = tempdir().unwrap();
        let schema_path = dir.path().join("schema.graphql");

        // Create a schema file
        fs::write(&schema_path, "type Query { hello: String }").unwrap();

        let source = SchemaSource::Single("schema.graphql".to_string());
        let merged_schema = "type Query { hello: String }";

        // Save to cache
        save_to_cache(dir.path(), &source, merged_schema).unwrap();

        // Load from cache
        let loaded = try_load_from_cache(dir.path(), &source);
        assert_eq!(loaded, Some(merged_schema.to_string()));

        // Modify file
        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&schema_path, "type Query { world: String }").unwrap();

        // Cache should be invalid
        let loaded = try_load_from_cache(dir.path(), &source);
        assert_eq!(loaded, None);
    }
}
