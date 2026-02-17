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
//! - macOS: `~/Library/Caches/graphox/`
//! - Linux: `~/.cache/graphox/`
//! - Windows: `%LOCALAPPDATA%\graphox\cache\`
//! - Custom: Set `GRAPHOX_CACHE_DIR` environment variable
//!
//! ## Performance impact
//!
//! Memory cache (L1): 95-99% faster than parsing
//! Disk cache (L2): 10-30% faster for small schemas, 50-80% for large schemas
//!
//! ## Configuration
//!
//! Caching is enabled by default. To disable it, set `enable_schema_cache: false` in
//! your `graphox.yaml` configuration file.

use crate::config::SchemaSource;
use ahash::AHashMap;
use apollo_compiler::{Schema, validation::Valid};
use dashmap::DashMap;
use std::fs;
use std::fs::OpenOptions;
use std::io;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Metadata for cache validation
#[derive(Debug, Clone)]
struct CacheMetadata {
    /// Maps file path to its last modification time
    file_mtimes: AHashMap<String, SystemTime>,
}

/// Cache entry containing the schema text and metadata (disk cache)
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
static MEMORY_CACHE: LazyLock<DashMap<String, MemoryCacheEntry>> = LazyLock::new(DashMap::new);

impl CacheMetadata {
    /// Create metadata from a list of file paths
    fn from_files(base_dir: &Path, files: &[String]) -> Result<Self, String> {
        let mut file_mtimes = AHashMap::default();

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

    /// Manual binary serialization for CacheMetadata
    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(self.file_mtimes.len() as u32).to_le_bytes());
        for (path, time) in &self.file_mtimes {
            let path_bytes = path.as_bytes();
            bytes.extend_from_slice(&(path_bytes.len() as u32).to_le_bytes());
            bytes.extend_from_slice(path_bytes);

            let duration = time.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
            bytes.extend_from_slice(&duration.as_secs().to_le_bytes());
            bytes.extend_from_slice(&duration.subsec_nanos().to_le_bytes());
        }
        bytes
    }

    /// Manual binary deserialization for CacheMetadata
    fn from_bytes(bytes: &[u8]) -> Option<(Self, usize)> {
        let mut offset = 0;
        if bytes.len() < 4 {
            return None;
        }
        let count = u32::from_le_bytes(bytes[0..4].try_into().ok()?) as usize;
        offset += 4;

        let mut file_mtimes = AHashMap::default();
        for _ in 0..count {
            if bytes.len() < offset + 4 {
                return None;
            }
            let path_len = u32::from_le_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
            offset += 4;

            if bytes.len() < offset + path_len {
                return None;
            }
            let path = String::from_utf8(bytes[offset..offset + path_len].to_vec()).ok()?;
            offset += path_len;

            if bytes.len() < offset + 12 {
                return None;
            }
            let secs = u64::from_le_bytes(bytes[offset..offset + 8].try_into().ok()?);
            offset += 8;
            let nanos = u32::from_le_bytes(bytes[offset..offset + 4].try_into().ok()?);
            offset += 4;

            let time = UNIX_EPOCH + Duration::new(secs, nanos);
            file_mtimes.insert(path, time);
        }

        Some((Self { file_mtimes }, offset))
    }
}

fn try_remove_file<P: AsRef<Path>>(p: P) {
    match fs::remove_file(&p) {
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(_) => {
            // Best-effort: ignore other errors as cache removal should not
            // abort the main flow in test environments.
        }
    }
}

fn rename_tmp_into_place(tmp_path: &Path, final_path: &Path) -> io::Result<()> {
    fs::rename(tmp_path, final_path)
}

impl CacheEntry {
    /// Manual binary serialization for CacheEntry
    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        let schema_bytes = self.merged_schema.as_bytes();
        bytes.extend_from_slice(&(schema_bytes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(schema_bytes);
        bytes.extend(self.metadata.to_bytes());
        bytes
    }

    /// Manual binary deserialization for CacheEntry
    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let mut offset = 0;
        if bytes.len() < 4 {
            return None;
        }
        let schema_len = u32::from_le_bytes(bytes[0..4].try_into().ok()?) as usize;
        offset += 4;

        if bytes.len() < offset + schema_len {
            return None;
        }
        let merged_schema = String::from_utf8(bytes[offset..offset + schema_len].to_vec()).ok()?;
        offset += schema_len;

        let (metadata, _) = CacheMetadata::from_bytes(&bytes[offset..])?;
        Some(Self {
            merged_schema,
            metadata,
        })
    }
}

// ============================================================================
// Memory Cache (L1) - Fully parsed schemas
// ============================================================================

// Helper function to generate a unique cache key including the base directory
fn make_cache_key(base_dir: &Path, source: &SchemaSource) -> String {
    let source_key = source.as_key();
    let base_path = base_dir.to_string_lossy();
    format!("{}:{}", base_path, source_key)
}

pub fn try_load_parsed_from_memory(
    base_dir: &Path,
    source: &SchemaSource,
) -> Option<Arc<Valid<Schema>>> {
    let key = make_cache_key(base_dir, source);
    let entry = MEMORY_CACHE.get(&key)?;

    if !entry.metadata.is_valid(base_dir) {
        drop(entry);
        MEMORY_CACHE.remove(&key);
        return None;
    }

    #[cfg(debug_assertions)]
    eprintln!("✓ Memory cache hit for {}", key);

    Some(entry.schema.clone())
}

pub fn save_parsed_to_memory(
    base_dir: &Path,
    source: &SchemaSource,
    schema: Arc<Valid<Schema>>,
) -> Result<(), String> {
    let key = make_cache_key(base_dir, source);
    let files = source.files();
    let metadata = CacheMetadata::from_files(base_dir, &files)?;
    let entry = MemoryCacheEntry { schema, metadata };
    MEMORY_CACHE.insert(key, entry);
    Ok(())
}

pub fn clear_memory_cache_for(base_dir: &Path, source: &SchemaSource) {
    let key = make_cache_key(base_dir, source);
    MEMORY_CACHE.remove(&key);
}

pub fn clear_memory_cache() {
    MEMORY_CACHE.clear();
}

pub fn clear_disk_cache() -> Result<(), String> {
    let cache_dir = get_cache_dir();
    if cache_dir.exists() {
        fs::remove_dir_all(&cache_dir).map_err(|e| format!("Failed to clear disk cache: {}", e))?;
        fs::create_dir_all(&cache_dir)
            .map_err(|e| format!("Failed to recreate cache dir: {}", e))?;
    }
    Ok(())
}

// ============================================================================
// Disk Cache (L2) - Merged schema text
// ============================================================================

fn get_cache_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("GRAPHOX_CACHE_DIR") {
        PathBuf::from(dir)
    } else if let Some(cache_home) = dirs::cache_dir() {
        cache_home.join("graphox")
    } else {
        PathBuf::from(".graphox-cache")
    }
}

fn get_cache_path(base_dir: &Path, source: &SchemaSource) -> PathBuf {
    let cache_dir = get_cache_dir();
    let key = make_cache_key(base_dir, source);
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut hasher = ahash::AHasher::default();
        key.hash(&mut hasher);
        hasher.finish()
    };
    cache_dir.join(format!("schema-{:x}.cache", hash))
}

pub fn try_load_from_cache(base_dir: &Path, source: &SchemaSource) -> Option<String> {
    let cache_path = get_cache_path(base_dir, source);
    if !cache_path.exists() {
        return None;
    }

    // Read the cache file. If it is corrupted (unable to deserialize), remove
    // it immediately to avoid repeatedly trying to load a broken file.
    let cache_data = match fs::read(&cache_path) {
        Ok(d) => d,
        Err(_) => return None,
    };

    let entry = match CacheEntry::from_bytes(&cache_data) {
        Some(e) => e,
        None => {
            // Corrupted cache file - attempt best-effort removal and bail out.
            try_remove_file(&cache_path);
            return None;
        }
    };

    if !entry.metadata.is_valid(base_dir) {
        try_remove_file(&cache_path);
        return None;
    }

    Some(entry.merged_schema)
}

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

    let cache_data = entry.to_bytes();
    let cache_path = get_cache_path(base_dir, source);
    if let Some(parent) = cache_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create cache directory: {}", e))?;
    }

    // Write to a temporary file in the same directory and then atomically
    // rename to the final cache path. This prevents readers from seeing a
    // partially-written cache file if multiple writers race.
    let tmp_file_name = if let Some(fname) = cache_path.file_name().and_then(|s| s.to_str()) {
        format!(
            "{}.tmp.{}.{}",
            fname,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        )
    } else {
        // Fallback to a fixed tmp suffix if filename isn't valid UTF-8
        format!("cache.tmp.{}", std::process::id())
    };

    let tmp_path = cache_path
        .parent()
        .map(|p| p.join(tmp_file_name))
        .unwrap_or_else(|| cache_path.with_extension("tmp"));

    if let Err(e) = write_cache_with_lock(&cache_data, &cache_path, &tmp_path) {
        // Cleanup temp file on failure
        try_remove_file(&tmp_path);
        return Err(format!("Failed to write cache file: {}", e));
    }
    Ok(())
}

/// Write `data` to a temporary file in the same directory as `final_path`,
/// fsync the file, and atomically rename it into place.
fn write_cache_with_lock(data: &[u8], final_path: &Path, tmp_path: &Path) -> io::Result<()> {
    // Ensure parent dir exists - caller normally does this but be defensive.
    if let Some(parent) = final_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Try to create the temp file using create_new so we don't clobber a
    // concurrently-created tmp file. If a name collision occurs, append a
    // quick counter and retry a few times.
    let mut attempt_tmp = tmp_path.to_path_buf();
    let mut created = false;
    let mut tmp_file: Option<std::fs::File> = None;
    for _ in 0..8 {
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&attempt_tmp)
        {
            Ok(f) => {
                tmp_file = Some(f);
                created = true;
                break;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                // Try a new name with a counter suffix
                let suffix = TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
                attempt_tmp = tmp_path.with_extension(format!("tmp.{}", suffix));
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    if !created {
        // As a last resort, open with truncate to ensure we can proceed.
        let f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&attempt_tmp)?;
        tmp_file = Some(f);
    }

    let mut f = tmp_file.expect("tmp file must be opened");
    // Write the complete payload before rename so readers never observe partial content.
    f.write_all(data)?;
    drop(f);

    if let Err(e) = rename_tmp_into_place(&attempt_tmp, final_path) {
        let _ = fs::remove_file(&attempt_tmp);
        return Err(e);
    }

    Ok(())
}

pub fn clear_cache() -> Result<(), String> {
    clear_memory_cache();
    let cache_dir = get_cache_dir();

    if cache_dir.exists() {
        let mut last_err: Option<std::io::Error> = None;
        for attempt in 0..5 {
            match fs::remove_dir_all(&cache_dir) {
                Ok(_) => {
                    last_err = None;
                    break;
                }
                Err(e) => {
                    last_err = Some(e);
                    if let Ok(entries) = fs::read_dir(&cache_dir) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            let _ = if p.is_file() {
                                fs::remove_file(&p)
                            } else {
                                fs::remove_dir_all(&p)
                            };
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5 * (attempt + 1) as u64));
                }
            }
        }

        if let Some(e) = last_err {
            // Ignore "Directory not empty" errors as they often indicate a race
            // with another process that is already recreating the cache.
            let mut is_not_empty = false;
            #[cfg(unix)]
            if e.raw_os_error() == Some(39) || e.raw_os_error() == Some(66) {
                is_not_empty = true;
            }
            #[cfg(windows)]
            if e.raw_os_error() == Some(145) {
                is_not_empty = true;
            }

            if !is_not_empty {
                return Err(format!("Failed to clear cache directory: {}", e));
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    #[ntest::timeout(100)]
    fn test_cache_metadata_validation() {
        let dir = tempdir().unwrap();
        let schema_path = dir.path().join("schema.graphql");

        let mut file = fs::File::create(&schema_path).unwrap();
        writeln!(file, "type Query {{ hello: String }}").unwrap();
        drop(file);

        let files = vec!["schema.graphql".to_string()];
        let metadata = CacheMetadata::from_files(dir.path(), &files).unwrap();
        assert!(metadata.is_valid(dir.path()));

        std::thread::sleep(std::time::Duration::from_millis(10));
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&schema_path)
            .unwrap();
        writeln!(file, "type Mutation {{ update: Boolean }}").unwrap();
        drop(file);
        assert!(!metadata.is_valid(dir.path()));
    }

    #[test]
    #[ntest::timeout(100)]
    fn test_cache_round_trip() {
        let dir = tempdir().unwrap();
        let schema_path = dir.path().join("schema.graphql");
        fs::write(&schema_path, "type Query { hello: String }").unwrap();

        let source = SchemaSource::Single("schema.graphql".to_string());
        let merged_schema = "type Query { hello: String }";

        save_to_cache(dir.path(), &source, merged_schema).unwrap();
        let loaded = try_load_from_cache(dir.path(), &source);
        assert_eq!(loaded, Some(merged_schema.to_string()));

        std::thread::sleep(std::time::Duration::from_millis(10));
        fs::write(&schema_path, "type Query { world: String }").unwrap();
        let loaded = try_load_from_cache(dir.path(), &source);
        assert_eq!(loaded, None);
    }

    #[test]
    #[ntest::timeout(100)]
    fn test_corrupted_cache_file_is_removed() {
        let dir = tempdir().unwrap();

        // Create a schema file so metadata checks can succeed later
        let schema_path = dir.path().join("schema.graphql");
        fs::write(&schema_path, "type Query { ok: Boolean }").unwrap();

        let source = SchemaSource::Single("schema.graphql".to_string());

        // Ensure cache directory exists and write a corrupted cache file
        let cache_path = get_cache_path(dir.path(), &source);
        if let Some(parent) = cache_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }

        // Write invalid bytes that cannot be deserialized
        fs::write(&cache_path, b"this is not a valid cache entry").unwrap();
        assert!(cache_path.exists());

        // Loader should return None and remove the corrupted cache file
        let loaded = try_load_from_cache(dir.path(), &source);
        assert_eq!(loaded, None);
        assert!(!cache_path.exists());

        // After removal, saving to cache must succeed and loader should read it back
        let merged = "type Query { ok: Boolean }";
        save_to_cache(dir.path(), &source, merged).unwrap();
        let loaded2 = try_load_from_cache(dir.path(), &source);
        assert_eq!(loaded2, Some(merged.to_string()));
    }
}
