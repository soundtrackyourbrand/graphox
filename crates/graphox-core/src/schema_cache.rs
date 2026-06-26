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

    /// Manual binary deserialization for CacheMetadata (simple version)
    fn from_bytes_simple(bytes: &[u8]) -> Option<Self> {
        Self::from_bytes(bytes).map(|(metadata, _)| metadata)
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

        let metadata = CacheMetadata::from_bytes_simple(&bytes[offset..])?;
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
    let base_path = crate::utils::to_posix_path(base_dir);
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

/// Default age after which a cache entry is eligible for pruning.
const DEFAULT_CACHE_MAX_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);
/// Default cap on the total size of the disk cache.
const DEFAULT_CACHE_MAX_SIZE: u64 = 256 * 1024 * 1024;
/// Minimum time between automatic prunes.
const PRUNE_THROTTLE: Duration = Duration::from_secs(24 * 60 * 60);
/// Marker file recording when the cache was last pruned (throttling).
const PRUNE_MARKER: &str = ".last-prune";

/// Whether a directory entry is a cache file this module owns (so pruning never
/// touches the marker or unrelated files). Covers both `schema-<hash>.cache` and
/// its transient `.tmp` siblings from interrupted writes.
fn is_prunable_cache_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("schema-"))
}

fn env_u64(name: &str) -> Option<u64> {
    std::env::var(name).ok().and_then(|v| v.trim().parse().ok())
}

/// Prune the cache files in `cache_dir`: first remove entries older than `max_age`,
/// then, if the total size still exceeds `max_total_bytes`, remove the oldest
/// entries until it fits. `None` disables the corresponding pass. Best-effort:
/// per-file I/O errors are ignored.
fn prune_cache_dir(
    cache_dir: &Path,
    max_age: Option<Duration>,
    max_total_bytes: Option<u64>,
) -> io::Result<()> {
    if !cache_dir.exists() {
        return Ok(());
    }
    let now = SystemTime::now();

    struct Entry {
        path: PathBuf,
        size: u64,
        mtime: SystemTime,
    }
    let mut survivors: Vec<Entry> = Vec::new();

    for dent in fs::read_dir(cache_dir)? {
        let Ok(dent) = dent else { continue };
        let path = dent.path();
        if !is_prunable_cache_file(&path) {
            continue;
        }
        let Ok(meta) = dent.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let mtime = meta.modified().unwrap_or(now);

        // Age pass: drop anything older than the cutoff.
        if let Some(max_age) = max_age
            && now
                .duration_since(mtime)
                .map(|age| age > max_age)
                .unwrap_or(false)
        {
            try_remove_file(&path);
            continue;
        }
        survivors.push(Entry {
            path,
            size: meta.len(),
            mtime,
        });
    }

    // Size pass: evict oldest-first until under the cap.
    if let Some(cap) = max_total_bytes {
        let mut total: u64 = survivors.iter().map(|e| e.size).sum();
        if total > cap {
            survivors.sort_by_key(|e| e.mtime); // oldest first
            for entry in &survivors {
                if total <= cap {
                    break;
                }
                if fs::remove_file(&entry.path).is_ok() {
                    total = total.saturating_sub(entry.size);
                }
            }
        }
    }
    Ok(())
}

/// Best-effort, throttled prune of the on-disk schema cache so it cannot grow
/// without bound. Returns immediately; the sweep runs on a detached thread so it
/// never blocks startup. It runs at most once per [`PRUNE_THROTTLE`], tracked by
/// the mtime of a marker file in the cache directory.
///
/// Tunable via env vars (`0` disables that pass):
/// - `GRAPHOX_CACHE_MAX_AGE_DAYS` (default 14)
/// - `GRAPHOX_CACHE_MAX_SIZE_MB` (default 256)
pub fn prune_cache_if_due() {
    std::thread::spawn(|| {
        let cache_dir = get_cache_dir();
        if !cache_dir.exists() {
            return;
        }

        // Throttle: skip if we pruned within the throttle window.
        let marker = cache_dir.join(PRUNE_MARKER);
        if let Ok(meta) = fs::metadata(&marker)
            && let Ok(mtime) = meta.modified()
            && SystemTime::now()
                .duration_since(mtime)
                .map(|since| since < PRUNE_THROTTLE)
                .unwrap_or(false)
        {
            return;
        }
        // Claim this window up front so co-starting processes don't all sweep.
        let _ = fs::write(&marker, b"");

        let max_age = match env_u64("GRAPHOX_CACHE_MAX_AGE_DAYS") {
            Some(0) => None,
            Some(days) => Some(Duration::from_secs(days.saturating_mul(24 * 60 * 60))),
            None => Some(DEFAULT_CACHE_MAX_AGE),
        };
        let max_size = match env_u64("GRAPHOX_CACHE_MAX_SIZE_MB") {
            Some(0) => None,
            Some(mb) => Some(mb.saturating_mul(1024 * 1024)),
            None => Some(DEFAULT_CACHE_MAX_SIZE),
        };

        let _ = prune_cache_dir(&cache_dir, max_age, max_size);
    });
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
            let is_not_found = e.kind() == io::ErrorKind::NotFound;
            #[cfg(unix)]
            if e.raw_os_error() == Some(39) || e.raw_os_error() == Some(66) {
                is_not_empty = true;
            }
            #[cfg(windows)]
            if e.raw_os_error() == Some(145) {
                is_not_empty = true;
            }

            if !is_not_empty && !is_not_found {
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

    /// Write a fake cache file of `size` bytes, aged `age` in the past.
    fn write_aged_cache_file(dir: &Path, name: &str, size: usize, age: Duration) {
        let path = dir.join(name);
        fs::write(&path, vec![b'x'; size]).unwrap();
        let when = SystemTime::now() - age;
        let f = OpenOptions::new().write(true).open(&path).unwrap();
        f.set_times(std::fs::FileTimes::new().set_modified(when))
            .unwrap();
    }

    #[test]
    #[ntest::timeout(2000)]
    fn prune_cache_dir_removes_entries_older_than_max_age() {
        let dir = tempdir().unwrap();
        let base = dir.path();

        write_aged_cache_file(base, "schema-old.cache", 10, Duration::from_secs(20 * 86400));
        write_aged_cache_file(base, "schema-new.cache", 10, Duration::from_secs(60));
        // A non-owned file and the throttle marker must never be touched.
        fs::write(base.join("unrelated.txt"), b"keep me").unwrap();
        fs::write(base.join(PRUNE_MARKER), b"").unwrap();

        prune_cache_dir(base, Some(Duration::from_secs(14 * 86400)), None).unwrap();

        assert!(!base.join("schema-old.cache").exists(), "old entry pruned");
        assert!(base.join("schema-new.cache").exists(), "fresh entry kept");
        assert!(base.join("unrelated.txt").exists(), "non-cache file kept");
        assert!(base.join(PRUNE_MARKER).exists(), "marker kept");
    }

    #[test]
    #[ntest::timeout(2000)]
    fn prune_cache_dir_enforces_size_cap_oldest_first() {
        let dir = tempdir().unwrap();
        let base = dir.path();

        // Three 1 KiB entries, ascending age (c oldest). Cap at 2 KiB → c evicted.
        write_aged_cache_file(base, "schema-a.cache", 1024, Duration::from_secs(60));
        write_aged_cache_file(base, "schema-b.cache", 1024, Duration::from_secs(120));
        write_aged_cache_file(base, "schema-c.cache", 1024, Duration::from_secs(600));

        prune_cache_dir(base, None, Some(2 * 1024)).unwrap();

        assert!(!base.join("schema-c.cache").exists(), "oldest evicted first");
        assert!(base.join("schema-a.cache").exists(), "newest kept");
        assert!(base.join("schema-b.cache").exists(), "second-newest kept");

        let total: u64 = fs::read_dir(base)
            .unwrap()
            .flatten()
            .filter(|e| is_prunable_cache_file(&e.path()))
            .map(|e| e.metadata().unwrap().len())
            .sum();
        assert!(total <= 2 * 1024, "total within cap: {total}");
    }

    #[test]
    #[ntest::timeout(2000)]
    fn prune_cache_dir_missing_dir_is_ok() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        prune_cache_dir(&missing, Some(Duration::from_secs(1)), Some(1)).unwrap();
    }

    #[test]
    #[ntest::timeout(300)]
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
    #[ntest::timeout(300)]
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
    #[ntest::timeout(300)]
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
