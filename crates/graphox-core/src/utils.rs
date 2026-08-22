use crate::document::DocumentState;
use crate::queries::*;
use crate::{Config, config::ProjectConfig};
use ahash::AHashMap;
use colored::*;
use ls_types::*;
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Instant;
use tree_sitter::StreamingIterator;

pub const SEMANTIC_TOKEN_LEGEND: &[SemanticTokenType] = &[
    SemanticTokenType::VARIABLE,
    SemanticTokenType::TYPE,
    SemanticTokenType::STRING,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::ENUM,
];

pub const DIAGNOSTIC_SOURCE: Option<&'static str> = Some("graphox");
const CANONICAL_PATH_CACHE_CAPACITY: usize = 256;
const PATH_CACHE_ORDER_SLACK: usize = 4;

static CANONICAL_PATH_CACHE: LazyLock<Mutex<BoundedPathCache<PathBuf>>> =
    LazyLock::new(|| Mutex::new(BoundedPathCache::new(CANONICAL_PATH_CACHE_CAPACITY)));

#[derive(Debug)]
pub(crate) struct BoundedPathCache<V> {
    entries: AHashMap<PathBuf, (V, u64)>,
    order: VecDeque<(PathBuf, u64)>,
    next_generation: u64,
    capacity: usize,
}

impl<V: Clone> BoundedPathCache<V> {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            entries: AHashMap::default(),
            order: VecDeque::new(),
            next_generation: 0,
            capacity,
        }
    }

    pub(crate) fn get(&mut self, key: &Path) -> Option<V> {
        let value = self.entries.get(key).map(|(value, _)| value.clone())?;
        let generation = self.bump_generation();
        if let Some((_, current_generation)) = self.entries.get_mut(key) {
            *current_generation = generation;
        }
        self.order.push_back((key.to_path_buf(), generation));
        self.compact_order_if_needed();
        Some(value)
    }

    pub(crate) fn insert(&mut self, key: PathBuf, value: V) {
        let generation = self.bump_generation();
        self.entries.insert(key.clone(), (value, generation));
        self.order.push_back((key, generation));
        self.evict_stale_entries();
        self.compact_order_if_needed();
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    fn bump_generation(&mut self) -> u64 {
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        self.next_generation
    }

    fn evict_stale_entries(&mut self) {
        while self.entries.len() > self.capacity {
            let Some((key, generation)) = self.order.pop_front() else {
                break;
            };

            let should_remove = self
                .entries
                .get(&key)
                .is_some_and(|(_, current_generation)| *current_generation == generation);
            if should_remove {
                self.entries.remove(&key);
            }
        }
    }

    fn compact_order_if_needed(&mut self) {
        let max_order_len = self
            .capacity
            .saturating_mul(PATH_CACHE_ORDER_SLACK)
            .max(self.capacity.saturating_add(1));
        if self.order.len() <= max_order_len {
            return;
        }

        let mut entries = self
            .entries
            .iter()
            .map(|(key, (_, generation))| (key.clone(), *generation))
            .collect::<Vec<_>>();
        entries.sort_unstable_by_key(|(_, generation)| *generation);
        self.order = entries.into_iter().collect();
    }
}

/// The filesystem path a document URI points at, owned.
///
/// `Uri::to_file_path` returns `Option<Cow<'_, Path>>` borrowed from a
/// percent-decoded copy of the URI, so nearly every caller ended up writing
/// `.into_owned()` or juggling the `Cow`. It also handles the Windows cases —
/// a `file:///c:/...` drive letter and a `file://server/...` share — which is
/// reason enough not to reimplement the conversion at each call site.
pub fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    uri.to_file_path().map(|p| p.into_owned())
}

/// The URI's path as text, percent-decoded.
///
/// `Uri::path()` hands back a percent-encoded `EStr`, so matching on it
/// directly disagrees with [`uri_to_path`] for any path containing an escape:
/// a file whose name ends in an encoded character would fail an extension
/// check that the decoded path passes. Decoding here keeps one meaning of "the
/// path" across the codebase.
pub fn uri_path_text(uri: &Uri) -> std::borrow::Cow<'_, str> {
    // Borrows when the path holds no escapes, which is the common case; the
    // owned form was allocating on every call and showed up as an 8% regression
    // in the is_schema_document_path benchmark.
    uri.path().decode().to_string_lossy()
}

pub fn flush_stdio() {
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();
}

pub fn canonicalize_cached(path: &Path) -> PathBuf {
    if !path.is_absolute() {
        return std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    }

    if let Ok(mut cache) = CANONICAL_PATH_CACHE.lock()
        && let Some(cached) = cache.get(path)
    {
        return cached;
    }

    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if let Ok(mut cache) = CANONICAL_PATH_CACHE.lock() {
        cache.insert(path.to_path_buf(), canonical.clone());
    }
    canonical
}

pub fn clear_canonical_path_cache() {
    if let Ok(mut cache) = CANONICAL_PATH_CACHE.lock() {
        *cache = BoundedPathCache::new(CANONICAL_PATH_CACHE_CAPACITY);
    }
}

const PROJECT_WALK_LOG_INTERVAL: usize = 1_000;

#[derive(Clone)]
pub struct WorkspaceScanInstrumentation {
    inner: Arc<WorkspaceScanInstrumentationInner>,
}

struct WorkspaceScanInstrumentationInner {
    start: Instant,
    path: PathBuf,
    writer: Mutex<std::io::BufWriter<File>>,
}

#[derive(Clone)]
pub struct ProjectWalkInstrumentation {
    instrumentation: WorkspaceScanInstrumentation,
    project_idx: usize,
    include_key: Arc<str>,
    codegen_enabled: bool,
    output_dir: Option<Arc<str>>,
    counters: Arc<ProjectWalkCounters>,
}

struct ProjectWalkCounters {
    entries: AtomicUsize,
    dirs: AtomicUsize,
    files: AtomicUsize,
    matched_files: AtomicUsize,
    skipped_dirs: AtomicUsize,
    next_log_at: AtomicUsize,
}

impl WorkspaceScanInstrumentation {
    pub fn create(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;
        Ok(Self {
            inner: Arc::new(WorkspaceScanInstrumentationInner {
                start: Instant::now(),
                path: path.to_path_buf(),
                writer: Mutex::new(std::io::BufWriter::new(file)),
            }),
        })
    }

    pub fn path(&self) -> PathBuf {
        self.inner.path.clone()
    }

    pub fn log_phase(&self, event: &str, message: impl AsRef<str>) {
        self.log_line(&format!("event={event} {}", message.as_ref()));
    }

    pub fn project_walk(
        &self,
        project_idx: usize,
        include_key: &str,
        codegen_enabled: bool,
        output_dir: Option<&str>,
    ) -> ProjectWalkInstrumentation {
        ProjectWalkInstrumentation {
            instrumentation: self.clone(),
            project_idx,
            include_key: Arc::from(include_key),
            codegen_enabled,
            output_dir: output_dir.map(Arc::from),
            counters: Arc::new(ProjectWalkCounters {
                entries: AtomicUsize::new(0),
                dirs: AtomicUsize::new(0),
                files: AtomicUsize::new(0),
                matched_files: AtomicUsize::new(0),
                skipped_dirs: AtomicUsize::new(0),
                next_log_at: AtomicUsize::new(PROJECT_WALK_LOG_INTERVAL),
            }),
        }
    }

    fn log_line(&self, message: &str) {
        let elapsed_ms = self.inner.start.elapsed().as_millis();
        if let Ok(mut writer) = self.inner.writer.lock() {
            let _ = writeln!(writer, "{elapsed_ms:>8}ms {message}");
            let _ = writer.flush();
        }
    }
}

impl ProjectWalkInstrumentation {
    pub fn start(
        &self,
        roots: &[PathBuf],
        include_patterns: &[String],
        exclude_patterns: &[String],
    ) {
        self.instrumentation.log_line(&format!(
            "event=project_walk_start project={} include={:?} codegen_enabled={} output_dir={:?} roots={:?} include_patterns={:?} exclude_patterns={:?}",
            self.project_idx,
            self.include_key,
            self.codegen_enabled,
            self.output_dir,
            roots,
            include_patterns,
            exclude_patterns
        ));
    }

    pub fn log_no_roots(&self, include_patterns: &[String], exclude_patterns: &[String]) {
        self.instrumentation.log_line(&format!(
            "event=project_walk_no_roots project={} include={:?} codegen_enabled={} output_dir={:?} include_patterns={:?} exclude_patterns={:?}",
            self.project_idx,
            self.include_key,
            self.codegen_enabled,
            self.output_dir,
            include_patterns,
            exclude_patterns
        ));
    }

    pub fn observe_dir(&self, path: &Path, skipped: bool) {
        self.counters.entries.fetch_add(1, Ordering::Relaxed);
        self.counters.dirs.fetch_add(1, Ordering::Relaxed);
        if skipped {
            self.counters.skipped_dirs.fetch_add(1, Ordering::Relaxed);
        }
        self.maybe_log_progress(path);
    }

    pub fn observe_file(&self, path: &Path, matched: bool) {
        self.counters.entries.fetch_add(1, Ordering::Relaxed);
        self.counters.files.fetch_add(1, Ordering::Relaxed);
        if matched {
            self.counters.matched_files.fetch_add(1, Ordering::Relaxed);
        }
        self.maybe_log_progress(path);
    }

    pub fn finish(&self, returned_files: usize) {
        self.instrumentation.log_line(&format!(
            "event=project_walk_complete project={} include={:?} entries={} dirs={} files={} matched_files={} skipped_dirs={} returned_files={}",
            self.project_idx,
            self.include_key,
            self.counters.entries.load(Ordering::Relaxed),
            self.counters.dirs.load(Ordering::Relaxed),
            self.counters.files.load(Ordering::Relaxed),
            self.counters.matched_files.load(Ordering::Relaxed),
            self.counters.skipped_dirs.load(Ordering::Relaxed),
            returned_files
        ));
    }

    fn maybe_log_progress(&self, path: &Path) {
        let entries = self.counters.entries.load(Ordering::Relaxed);
        let mut next_log_at = self.counters.next_log_at.load(Ordering::Relaxed);
        while entries >= next_log_at {
            let new_next = next_log_at.saturating_add(PROJECT_WALK_LOG_INTERVAL);
            match self.counters.next_log_at.compare_exchange(
                next_log_at,
                new_next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    self.instrumentation.log_line(&format!(
                        "event=project_walk_progress project={} include={:?} entries={} dirs={} files={} matched_files={} skipped_dirs={} last_path={:?}",
                        self.project_idx,
                        self.include_key,
                        entries,
                        self.counters.dirs.load(Ordering::Relaxed),
                        self.counters.files.load(Ordering::Relaxed),
                        self.counters.matched_files.load(Ordering::Relaxed),
                        self.counters.skipped_dirs.load(Ordering::Relaxed),
                        path
                    ));
                    break;
                }
                Err(observed) => next_log_at = observed,
            }
        }
    }
}

#[repr(u32)]
pub enum SemanticTokenKind {
    Variable = 0,
    Type = 1,
    String = 2,
    Keyword = 3,
    Property = 4,
    Function = 5,
    Enum = 6,
}

/// Converts an apollo-compiler location to an LSP range
pub fn apollo_location_to_range(
    location: &Option<apollo_compiler::parser::SourceSpan>,
    source_file: &apollo_compiler::parser::SourceFile,
    encoding: PositionEncodingKind,
) -> Option<Range> {
    let loc = location.as_ref()?;
    let start_offset = loc.offset();
    let end_offset = start_offset + loc.node_len();

    let start_pos = offset_to_position(source_file.source_text(), start_offset, encoding.clone())?;
    let end_pos = offset_to_position(source_file.source_text(), end_offset, encoding)?;

    Some(Range {
        start: start_pos,
        end: end_pos,
    })
}

fn offset_to_position(
    source: &str,
    offset: usize,
    encoding: PositionEncodingKind,
) -> Option<Position> {
    if offset > source.len() || !source.is_char_boundary(offset) {
        return None;
    }

    let mut line = 0;
    let mut character = 0;
    let mut current_offset = 0;

    for (i, c) in source.char_indices() {
        if i >= offset {
            break;
        }
        if c == '\n' {
            line += 1;
            character = 0;
        } else if encoding == PositionEncodingKind::UTF16 {
            character += c.len_utf16() as u32;
        } else if encoding == PositionEncodingKind::UTF8 {
            character += c.len_utf8() as u32;
        } else {
            character += 1;
        }
        current_offset = i + c.len_utf8();
    }

    if current_offset < offset && offset <= source.len() {
        // Handle case where offset is at the very end or after last character processed
        let remaining = &source[current_offset..offset];
        for c in remaining.chars() {
            if c == '\n' {
                line += 1;
                character = 0;
            } else if encoding == PositionEncodingKind::UTF16 {
                character += c.len_utf16() as u32;
            } else if encoding == PositionEncodingKind::UTF8 {
                character += c.len_utf8() as u32;
            } else {
                character += 1;
            }
        }
    }

    Some(Position { line, character })
}

pub fn is_relevant_file(path: &Path) -> bool {
    (|| {
        if path
            .components()
            .any(|c| c.as_os_str() == "node_modules" || c.as_os_str() == ".git")
        {
            return false;
        }

        let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        let is_ext_relevant = matches!(
            ext,
            "graphql"
                | "graphqls"
                | "gql"
                | "ts"
                | "tsx"
                | "mts"
                | "cts"
                | "js"
                | "jsx"
                | "mjs"
                | "cjs"
        );

        if !is_ext_relevant {
            return false;
        }

        true
    })()
}

pub fn has_generated_header(content: &str) -> bool {
    content.starts_with("// @generated")
        || content.starts_with("/* @generated */")
        || content.starts_with("// Code generated by")
        || content.starts_with("/** @generated */")
        // A *header*: the sentence has to open a comment near the top of the file.
        // Matching it anywhere would classify a hand-written file that merely mentions
        // it — a doc comment, a JSX string, a codegen template — as generated, dropping
        // it from the scan. Now that codegen prunes outputs with no source document,
        // that would delete the file's live generated output.
        || content
            .lines()
            .take(GENERATED_HEADER_MAX_LINES)
            .any(is_generated_header_line)
}

/// How far into a file the free-text generated marker still counts as a header.
/// graphox emits it on the third line, after the tslint/eslint pragmas; the slack
/// leaves room for another tool's licence or pragma preamble.
const GENERATED_HEADER_MAX_LINES: usize = 10;

const GENERATED_HEADER_SENTENCE: &str =
    "This file was automatically generated and should not be edited.";

fn is_generated_header_line(line: &str) -> bool {
    let line = line.trim_start();
    let body = ["//", "/*", "*", "#", "--"]
        .iter()
        .find_map(|marker| line.strip_prefix(marker))
        .unwrap_or(line);
    body.trim_start().starts_with(GENERATED_HEADER_SENTENCE)
}

pub fn get_glob_root(pattern: &str) -> PathBuf {
    let path = Path::new(pattern);
    let mut root = PathBuf::new();
    let components: Vec<_> = path.components().collect();
    for component in components {
        let s = component.as_os_str().to_str().unwrap_or("");
        if s.contains('*') || s.contains('?') || s.contains('[') || s.contains('{') {
            break;
        }
        // If it looks like a file (has an extension), don't include it in the root
        if Path::new(s).extension().is_some() {
            break;
        }
        root.push(component);
    }
    root
}

fn should_skip_project_walk_dir(
    path: &Path,
    base_dir: &Path,
    exclude_set: &globset::GlobSet,
) -> bool {
    if path.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some("node_modules" | ".git" | "__generated__")
        )
    }) {
        return true;
    }

    if exclude_set.is_match(path) || exclude_set.is_match(to_posix_path(path)) {
        return true;
    }

    pathdiff::diff_paths(path, base_dir).is_some_and(|rel_to_base| {
        exclude_set.is_match(&rel_to_base) || exclude_set.is_match(to_posix_path(&rel_to_base))
    })
}

pub fn get_project_files(
    include_patterns: &[String],
    exclude_patterns: &[String],
    base_dir: &Path,
    instrumentation: Option<ProjectWalkInstrumentation>,
) -> Vec<PathBuf> {
    use globset::{Glob, GlobSetBuilder};
    use ignore::WalkBuilder;

    let mut include_builder = GlobSetBuilder::new();
    let mut roots = Vec::new();
    let mut direct_files = Vec::new();

    for p in include_patterns {
        let p_clean = p.strip_prefix("./").unwrap_or(p);

        let is_glob = p_clean.contains('*')
            || p_clean.contains('?')
            || p_clean.contains('[')
            || p_clean.contains('{');

        if !is_glob {
            let path = base_dir.join(p_clean);
            if path.is_file() {
                direct_files.push(path);
                continue;
            }
            if path.is_dir() {
                roots.push(path.clone());
                let mut p_glob = p_clean.to_string();
                if !p_glob.ends_with('/') && !p_glob.is_empty() {
                    p_glob.push('/');
                }
                p_glob.push_str("**/*");
                if let Ok(g) = Glob::new(&p_glob) {
                    include_builder.add(g);
                }
                continue;
            }
        }

        if let Ok(g) = Glob::new(p) {
            include_builder.add(g);
        }
        if p_clean != p
            && let Ok(g) = Glob::new(p_clean)
        {
            include_builder.add(g);
        }

        let root = get_glob_root(p_clean);
        let root_path = if root.as_os_str().is_empty() {
            base_dir.to_path_buf()
        } else {
            base_dir.join(root)
        };
        if root_path.exists() {
            roots.push(root_path);
        }
    }

    let mut exclude_builder = GlobSetBuilder::new();
    for p in exclude_patterns {
        let p_clean = p.strip_prefix("./").unwrap_or(p);
        if let Ok(g) = Glob::new(p_clean) {
            exclude_builder.add(g);
        }
        if p != p_clean
            && let Ok(g) = Glob::new(p)
        {
            exclude_builder.add(g);
        }
    }

    let include_set = include_builder
        .build()
        .unwrap_or_else(|_| GlobSetBuilder::new().build().unwrap());
    let exclude_set = exclude_builder
        .build()
        .unwrap_or_else(|_| GlobSetBuilder::new().build().unwrap());

    let (tx, rx) = std::sync::mpsc::channel();

    if !roots.is_empty() {
        roots.sort();
        let mut unique_roots: Vec<PathBuf> = Vec::new();
        for root in roots {
            if !unique_roots.iter().any(|r| path_starts_with(&root, r)) {
                unique_roots.push(root);
            }
        }

        if let Some(instrumentation) = instrumentation.as_ref() {
            instrumentation.start(&unique_roots, include_patterns, exclude_patterns);
        }

        let mut walk_builder = WalkBuilder::new(&unique_roots[0]);
        for root in &unique_roots[1..] {
            walk_builder.add(root);
        }

        let walk = walk_builder
            .add_custom_ignore_filename(".graphqlignore")
            .hidden(false)
            .follow_links(true)
            .build_parallel();

        let include_set_ref = &include_set;
        let exclude_set_ref = &exclude_set;
        let instrumentation_ref = instrumentation.clone();

        walk.run(|| {
            let tx = tx.clone();
            let instrumentation = instrumentation_ref.clone();
            Box::new(move |entry| {
                if let Ok(entry) = entry {
                    let path = entry.path();
                    if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                        let should_skip =
                            should_skip_project_walk_dir(path, base_dir, exclude_set_ref);
                        if let Some(instrumentation) = instrumentation.as_ref() {
                            instrumentation.observe_dir(path, should_skip);
                        }
                        if should_skip {
                            return ignore::WalkState::Skip;
                        }
                    }

                    if entry.file_type().is_some_and(|ft| ft.is_file()) {
                        let mut matched_file = false;
                        let mut matched = include_set_ref.is_match(path)
                            || include_set_ref.is_match(to_posix_path(path));

                        if !matched && is_relevant_file(path) {
                            let abs_path = canonicalize_cached(path);
                            matched = include_set_ref.is_match(&abs_path)
                                || include_set_ref.is_match(to_posix_path(&abs_path));
                        }

                        if !matched
                            && is_relevant_file(path)
                            && let Some(rel_to_base) = pathdiff::diff_paths(path, base_dir)
                        {
                            let posix_rel_path = to_posix_path(&rel_to_base);
                            matched = include_set_ref.is_match(&rel_to_base)
                                || include_set_ref.is_match(posix_rel_path);
                        }

                        if !matched
                            && is_relevant_file(path)
                            && let Some(file_name) = path.file_name()
                            && include_set_ref.is_match(file_name)
                        {
                            matched = true;
                        }

                        if matched && is_relevant_file(path) {
                            let mut excluded = exclude_set_ref.is_match(path)
                                || exclude_set_ref.is_match(to_posix_path(path));
                            if !excluded
                                && let Some(rel_to_base) = pathdiff::diff_paths(path, base_dir)
                                && (exclude_set_ref.is_match(&rel_to_base)
                                    || exclude_set_ref.is_match(to_posix_path(&rel_to_base)))
                            {
                                excluded = true;
                            }

                            if !excluded {
                                matched_file = true;
                                let send_path = if cfg!(windows) {
                                    canonicalize_cached(path)
                                } else {
                                    path.to_owned()
                                };
                                let _ = tx.send(send_path);
                            }
                        }

                        if let Some(instrumentation) = instrumentation.as_ref() {
                            instrumentation.observe_file(path, matched_file);
                        }
                    }
                }
                ignore::WalkState::Continue
            })
        });
    } else if let Some(instrumentation) = instrumentation.as_ref() {
        instrumentation.log_no_roots(include_patterns, exclude_patterns);
    }

    drop(tx);
    let mut files: Vec<PathBuf> = rx.into_iter().collect();

    files.extend(direct_files);

    files.sort();
    files.dedup();
    if let Some(instrumentation) = instrumentation.as_ref() {
        instrumentation.finish(files.len());
    }
    files
}

pub fn output_dir_requires_surgical_handling(
    base_dir: &Path,
    include_patterns: &[String],
    output_dir: &Path,
) -> bool {
    include_patterns.iter().any(|pattern| {
        let include_root = get_glob_root(pattern);
        let abs_include_root = base_dir.join(&include_root);
        output_dir == abs_include_root || path_starts_with(&abs_include_root, output_dir)
    })
}

/// True for a path graphox emits per source document (`<name>.codegen.ts`).
pub fn is_codegen_output_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".codegen.ts"))
}

/// Removes `*.codegen.ts` files under `dir` that the current run did not account for,
/// i.e. outputs whose source document was deleted or no longer contains any GraphQL.
/// Without this a plain (non-`--clean`) run leaves them on disk, where they keep
/// importing symbols from the outputs that *were* regenerated and break `tsc`.
///
/// `keep` holds every output path the run is responsible for — including files it
/// attempted but failed to generate, which must survive a transiently broken document.
/// Both sides are compared canonically and a candidate that cannot be canonicalized is
/// left alone: pruning must never delete a file it can't positively identify as an
/// orphan. Callers are responsible for the coarser safety gate — only pass a `dir`
/// whose every contributing project ran to completion.
///
/// With `recursive` the whole tree under `dir` is swept (a project `output_dir`, which
/// mirrors the source tree); without it only `dir` itself (co-located outputs, where
/// the surrounding tree is not graphox-owned). `remove_empty_dirs` additionally drops
/// directories left empty by the sweep.
fn prune_orphaned_codegen_in_dir(
    dir: &Path,
    recursive: bool,
    remove_empty_dirs: bool,
    keep: &ahash::AHashSet<PathBuf>,
) -> Vec<PathBuf> {
    if !dir.is_dir() {
        return Vec::new();
    }

    let keep_canon: ahash::AHashSet<PathBuf> = keep
        .iter()
        .filter_map(|p| std::fs::canonicalize(p).ok())
        .collect();

    let mut builder = ignore::WalkBuilder::new(dir);
    builder
        .hidden(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false);
    if !recursive {
        builder.max_depth(Some(1));
    }

    let mut removed = Vec::new();
    for entry in builder.build().flatten() {
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        if !is_codegen_output_file(path) {
            continue;
        }
        let Ok(canonical) = std::fs::canonicalize(path) else {
            continue;
        };
        if keep_canon.contains(&canonical) {
            continue;
        }
        if std::fs::remove_file(path).is_ok() {
            removed.push(path.to_path_buf());
        }
    }

    if remove_empty_dirs {
        let mut parents: Vec<PathBuf> = removed
            .iter()
            .filter_map(|p| p.parent().map(Path::to_path_buf))
            .collect();
        parents.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
        parents.dedup();
        for parent in parents {
            // `remove_dir` only succeeds on an empty directory, so this stops at the
            // first ancestor that still holds anything.
            let mut current = parent;
            while current != dir
                && path_starts_with(&current, dir)
                && std::fs::remove_dir(&current).is_ok()
            {
                match current.parent() {
                    Some(p) => current = p.to_path_buf(),
                    None => break,
                }
            }
        }
    }

    removed
}

/// Deletes generated files that no longer correspond to a source document, and returns
/// what was removed.
///
/// Codegen is purely source-driven: it maps each existing document to an output path
/// and writes it, so a source that was deleted — or that no longer contains any GraphQL
/// — leaves its `.codegen.ts` behind. The entrypoint and manifest are rewritten
/// wholesale and self-heal, but the orphan keeps importing symbols from the outputs
/// that *were* regenerated, and breaks `tsc` as soon as one of them is renamed.
///
/// `project_outputs` maps project index to every output path that project claimed this
/// run, and doubles as the safety gate: a project missing from it did not run to
/// completion (codegen disabled, no files, an unreadable source, or an error), so
/// anything it might have written is left alone rather than swept against an incomplete
/// keep-set.
///
/// The keep-set is deliberately GLOBAL rather than per-directory. Output paths mirror
/// the source tree under a project's `output_dir` ([`get_output_path`]), so two projects
/// routinely write into the same directory without declaring the same `output_dir` — a
/// nested `output_dir`, or a project with none at all whose outputs land under another
/// project's. Partitioning the keep-set by declaring project would make each sweep
/// delete the other project's live outputs. A file is an orphan only if *no* project
/// claimed it.
///
/// Blocking works on overlap in both directions for the same reason: a blocked
/// project's outputs can sit under a live project's directory or above it, so any
/// sweep root that overlaps a blocked project's `output_dir` is skipped. A blocked
/// project with no `output_dir` could have written anywhere under `base_dir`, so it
/// suppresses the whole pass.
pub fn prune_orphaned_outputs(
    config: &Config,
    project_outputs: &AHashMap<usize, Vec<PathBuf>>,
) -> Vec<PathBuf> {
    let mut keep: ahash::AHashSet<PathBuf> = ahash::AHashSet::default();
    let mut recursive_roots: Vec<PathBuf> = Vec::new();
    let mut colocated_dirs: Vec<PathBuf> = Vec::new();
    let mut blocked_dirs: Vec<PathBuf> = Vec::new();
    let mut include_patterns: Vec<String> = Vec::new();

    for (idx, project) in config.projects().iter().enumerate() {
        // Every project's include patterns feed the surgical check, since any of them
        // can turn a sweep root into a directory the user also owns.
        include_patterns.extend(project.include().patterns());

        let outputs = project_outputs
            .get(&idx)
            .filter(|_| config.get_codegen_config(Some(project)).prune_orphans());

        match (project.output_dir(), outputs) {
            (Some(out_dir), Some(paths)) => {
                let abs_out_dir = config.base_dir().join(out_dir);
                recursive_roots.push(abs_out_dir.canonicalize().unwrap_or(abs_out_dir));
                keep.extend(paths.iter().cloned());
            }
            (Some(out_dir), None) => {
                let abs_out_dir = config.base_dir().join(out_dir);
                blocked_dirs.push(abs_out_dir.canonicalize().unwrap_or(abs_out_dir));
            }
            (None, Some(paths)) => {
                // Without an output_dir the outputs land under `base_dir` mirroring the
                // path below each include root, among files graphox does not own, so
                // only the directories this run wrote into are swept — never the tree.
                colocated_dirs.extend(
                    paths
                        .iter()
                        .filter_map(|p| p.parent().map(Path::to_path_buf)),
                );
                keep.extend(paths.iter().cloned());
            }
            (None, None) => return Vec::new(),
        }
    }

    let overlaps_blocked = |dir: &PathBuf| {
        blocked_dirs.iter().any(|blocked| {
            dir == blocked || path_starts_with(dir, blocked) || path_starts_with(blocked, dir)
        })
    };
    recursive_roots.retain(|dir| !overlaps_blocked(dir));
    colocated_dirs.retain(|dir| !overlaps_blocked(dir));
    recursive_roots.sort();
    recursive_roots.dedup();
    colocated_dirs.sort();
    colocated_dirs.dedup();

    let mut removed: Vec<PathBuf> = Vec::new();

    for dir in &recursive_roots {
        // In the surgical case the output dir doubles as an include root, so the
        // directory structure is the user's — sweep the files but leave it standing.
        let is_surgical =
            output_dir_requires_surgical_handling(config.base_dir(), &include_patterns, dir);
        removed.extend(prune_orphaned_codegen_in_dir(
            dir,
            true,
            !is_surgical,
            &keep,
        ));
    }

    for dir in &colocated_dirs {
        // A recursive root already swept this directory if it contains it.
        if recursive_roots
            .iter()
            .any(|root| path_starts_with(dir, root))
        {
            continue;
        }
        removed.extend(prune_orphaned_codegen_in_dir(dir, false, false, &keep));
    }

    removed.sort();
    removed
}

pub fn get_project_scan_files(
    config: &Config,
    project: &ProjectConfig,
    instrumentation: Option<ProjectWalkInstrumentation>,
) -> Vec<PathBuf> {
    let abs_includes: Vec<String> = project
        .include()
        .patterns()
        .iter()
        .map(|pattern| {
            let abs = config.base_dir().join(pattern);
            to_posix_path(&abs)
        })
        .collect();

    let mut abs_excludes: Vec<String> = project
        .exclude()
        .map(|exclude| exclude.patterns())
        .unwrap_or_default()
        .iter()
        .map(|pattern| {
            let abs = config.base_dir().join(pattern);
            to_posix_path(&abs)
        })
        .collect();

    if let Some(output_dir) = project.output_dir() {
        let abs_output_dir = config.base_dir().join(output_dir);
        let include_patterns = project.include().patterns();
        if !output_dir_requires_surgical_handling(
            config.base_dir(),
            &include_patterns,
            &abs_output_dir,
        ) {
            abs_excludes.push(to_posix_path(&abs_output_dir));
            abs_excludes.push(output_dir.to_string());
        }
    }

    abs_excludes.push("**/__generated__".to_string());
    abs_excludes.push("**/__generated__/**".to_string());

    get_project_files(
        &abs_includes,
        &abs_excludes,
        config.base_dir(),
        instrumentation,
    )
}

pub fn get_gitignore_matcher(base_dir: &Path) -> ignore::gitignore::Gitignore {
    let mut builder = ignore::gitignore::GitignoreBuilder::new(base_dir);

    // Recursively find and add all .gitignore files in the project so the matcher
    // handles nested gitignores correctly.
    //
    // `git_ignore(true)` makes the walk itself honour .gitignore, so it prunes
    // ignored subtrees (e.g. `node_modules`, build output) instead of descending
    // into them. On a large JS monorepo that is the difference between walking
    // ~500k files and ~6k: this matcher is built synchronously in `Backend::new`
    // and otherwise blocks the LSP `initialize` response for seconds.
    //
    // We intentionally do not collect `.gitignore` files that live *inside* an
    // ignored directory: those subtrees are already excluded by the ignore rule
    // that ignores the directory, so their inner rules can never change a match.
    // `hidden(false)` is required so the `.gitignore` files themselves (a hidden
    // filename) are still visited.
    for entry in ignore::WalkBuilder::new(base_dir)
        .hidden(false)
        .git_ignore(true)
        .build()
    {
        if let Ok(entry) = entry
            && entry.file_name() == ".gitignore"
        {
            let _ = builder.add(entry.path());
        }
    }

    builder
        .build()
        .unwrap_or_else(|_| ignore::gitignore::Gitignore::empty())
}

pub fn is_path_ignored(path: &Path, matcher: &ignore::gitignore::Gitignore) -> bool {
    matcher.matched(path, path.is_dir()).is_ignore()
}

pub fn find_package_root(start_path: &Path) -> Option<PathBuf> {
    let mut current = if start_path.is_dir() {
        start_path.to_path_buf()
    } else {
        start_path.parent()?.to_path_buf()
    };

    loop {
        if current.join("package.json").exists() {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

/// Helper to compare two paths for equality, handling platform-specific quirks
/// like Windows UNC prefixes and case-insensitivity.
pub fn paths_match(a: Option<&Path>, b: Option<&Path>) -> bool {
    match (a, b) {
        (Some(pa), Some(pb)) => {
            if pa == pb {
                return true;
            }

            #[cfg(windows)]
            {
                let sa = pa.to_string_lossy();
                let sb = pb.to_string_lossy();

                let ca = normalize_windows_path(&sa);
                let cb = normalize_windows_path(&sb);
                return ca.eq_ignore_ascii_case(&cb);
            }

            #[cfg(not(windows))]
            {
                false
            }
        }
        (None, None) => true,
        _ => false,
    }
}

pub fn path_starts_with(path: &Path, prefix: &Path) -> bool {
    if path.starts_with(prefix) {
        return true;
    }

    #[cfg(target_os = "macos")]
    {
        use std::ffi::OsStr;
        let mut p_comps = path.components();
        let mut pre_comps = prefix.components();

        // Skip /private if present in only one of them
        let mut p_first = p_comps.next();
        let mut pre_first = pre_comps.next();

        if let (Some(std::path::Component::RootDir), Some(std::path::Component::RootDir)) =
            (p_first, pre_first)
        {
            p_first = p_comps.next();
            pre_first = pre_comps.next();

            if let (
                Some(std::path::Component::Normal(p_n)),
                Some(std::path::Component::Normal(pre_n)),
            ) = (p_first, pre_first)
            {
                if p_n == OsStr::new("private") && pre_n != OsStr::new("private") {
                    p_first = p_comps.next();
                } else if p_n != OsStr::new("private") && pre_n == OsStr::new("private") {
                    pre_first = pre_comps.next();
                }
            }
        }

        // Compare remaining components
        while let (Some(p_c), Some(pre_c)) = (p_first, pre_first) {
            if p_c != pre_c {
                return false;
            }
            p_first = p_comps.next();
            pre_first = pre_comps.next();
        }

        pre_first.is_none()
    }

    #[cfg(windows)]
    {
        use std::path::Component;
        let mut p_comps = path.components();
        let mut pre_comps = prefix.components();

        loop {
            match (p_comps.next(), pre_comps.next()) {
                (Some(p), Some(pre)) => {
                    let match_comp = match (p, pre) {
                        (Component::Normal(s1), Component::Normal(s2)) => s1
                            .to_string_lossy()
                            .eq_ignore_ascii_case(&s2.to_string_lossy()),
                        (Component::Prefix(p1), Component::Prefix(p2)) => {
                            let normalize_prefix = |kind: std::path::Prefix| {
                                use std::path::Prefix::*;
                                match kind {
                                    VerbatimDisk(d) | Disk(d) => {
                                        format!("{}:", (d as char).to_uppercase())
                                    }
                                    VerbatimUNC(s1, s2) | UNC(s1, s2) => {
                                        format!(
                                            "\\\\{}\\{}",
                                            s1.to_string_lossy(),
                                            s2.to_string_lossy()
                                        )
                                    }
                                    Verbatim(s) | DeviceNS(s) => s.to_string_lossy().to_string(),
                                }
                            };
                            normalize_prefix(p1.kind())
                                .eq_ignore_ascii_case(&normalize_prefix(p2.kind()))
                        }
                        (c1, c2) => c1 == c2,
                    };
                    if !match_comp {
                        return false;
                    }
                }
                (_, None) => return true,
                (None, Some(_)) => return false,
            }
        }
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    {
        false
    }
}

pub fn get_output_path(
    path: &Path,
    base_dir: &Path,
    output_dir: Option<&Path>,
    include_prefix: Option<&Path>,
) -> PathBuf {
    let mut output_path = base_dir.to_path_buf();

    // Canonicalize base_dir and path to handle symlinks (like /tmp -> /private/tmp on macOS)
    let abs_base_dir = canonicalize_cached(base_dir);
    let abs_path = canonicalize_cached(path);

    let diff_target = if let Some(prefix) = include_prefix {
        canonicalize_cached(&base_dir.join(prefix))
    } else {
        abs_base_dir
    };

    let rel_path = if let Ok(rel) = abs_path.strip_prefix(&diff_target) {
        rel.to_path_buf()
    } else {
        #[cfg(windows)]
        {
            // Robustly calculate relative path by stripping Windows prefixes if necessary
            let s_abs = abs_path.to_string_lossy();
            let s_target = diff_target.to_string_lossy();

            let clean_abs = normalize_windows_path(&s_abs);
            let clean_target = normalize_windows_path(&s_target);

            pathdiff::diff_paths(&clean_abs, &clean_target).unwrap_or_else(|| abs_path.clone())
        }

        #[cfg(not(windows))]
        {
            pathdiff::diff_paths(&abs_path, &diff_target).unwrap_or_else(|| abs_path.clone())
        }
    };

    if let Some(out_dir) = output_dir {
        output_path.push(out_dir);
    }
    output_path.push(rel_path);

    output_path.set_extension("codegen.ts");
    output_path
}

pub fn merge_schema_texts(texts: &[String]) -> String {
    let total_len: usize = texts.iter().map(|s| s.len() + 1).sum();
    let mut merged = String::with_capacity(total_len);
    let mut seen_base = ahash::AHashSet::default();
    let mut seen_schema = false;
    let mut seen_root_op_names = ahash::AHashSet::default();

    let mut parser = tree_sitter::Parser::new();
    let language: tree_sitter::Language = tree_sitter_graphql::LANGUAGE.into();
    if let Err(e) = parser.set_language(&language) {
        eprintln!(
            "{}: Failed to set GraphQL language: {}",
            "ERROR".red(),
            e.to_string().red()
        );
        for text in texts {
            merged.push_str(text);
            merged.push('\n');
        }
        return merged;
    }

    let query = crate::queries::GQL_MERGE_QUERY_CACHE.get_or_init(|| {
        tree_sitter::Query::new(&language, crate::queries::GQL_MERGE_QUERY).unwrap()
    });
    let name_idx = query.capture_index_for_name("name").unwrap();
    let type_def_idx = query.capture_index_for_name("type_def").unwrap();
    let schema_def_idx = query.capture_index_for_name("schema_def").unwrap();
    let operation_idx = query.capture_index_for_name("operation").unwrap();
    let named_type_idx = query.capture_index_for_name("named_type").unwrap();
    let root_op_idx = query.capture_index_for_name("root_op").unwrap();
    let mut cursor = tree_sitter::QueryCursor::new();

    for text in texts {
        let tree = if let Some(t) = parser.parse(text, None) {
            t
        } else {
            merged.push_str(text);
            merged.push('\n');
            continue;
        };
        let root = tree.root_node();

        // One pass to collect all info
        let mut collected_matches = Vec::new();
        {
            let mut matches_iter = cursor.matches(query, root, text.as_bytes());
            while let Some(m) = matches_iter.next() {
                let mut name_node = None;
                let mut container_node = None;
                let mut is_schema_def = false;
                let mut is_root_op = false;
                let mut operation_node = None;
                let mut named_type_node = None;

                for cap in m.captures {
                    if cap.index == name_idx {
                        name_node = Some(cap.node);
                    } else if cap.index == type_def_idx {
                        container_node = Some(cap.node);
                    } else if cap.index == schema_def_idx {
                        is_schema_def = true;
                    } else if cap.index == root_op_idx {
                        is_root_op = true;
                    } else if cap.index == operation_idx {
                        operation_node = Some(cap.node);
                    } else if cap.index == named_type_idx {
                        named_type_node = Some(cap.node);
                    }
                }

                if let Some(container) = container_node {
                    collected_matches.push((
                        container,
                        is_schema_def,
                        is_root_op,
                        name_node,
                        operation_node,
                        named_type_node,
                    ));
                }
            }
        }

        let mut modifications = Vec::new();
        let mut schema_block_indices = Vec::new();

        // Map root ops to their containing schema blocks
        for (idx, (container, is_schema, _, _, _, _)) in collected_matches.iter().enumerate() {
            if *is_schema {
                schema_block_indices.push((
                    idx,
                    container.start_byte(),
                    container.end_byte(),
                    Vec::new(),
                ));
            }
        }

        for (idx, (container, _, is_root_op, _, _, _)) in collected_matches.iter().enumerate() {
            if *is_root_op {
                for block in schema_block_indices.iter_mut() {
                    if container.start_byte() >= block.1 && container.end_byte() <= block.2 {
                        block.3.push(idx);
                        break;
                    }
                }
            }
        }

        let mut root_ops_to_remove = ahash::AHashSet::default();
        let mut handled_schema_blocks = ahash::AHashSet::default();

        for (m_idx, (container, is_schema, is_root_op, name_node, _op_node, _ty_node)) in
            collected_matches.iter().enumerate()
        {
            if *is_schema {
                if handled_schema_blocks.contains(&m_idx) {
                    continue;
                }

                let block = schema_block_indices.iter().find(|b| b.0 == m_idx).unwrap();
                let mut unique_ops_in_block = Vec::new();
                for &r_idx in &block.3 {
                    let (_, _, _, _, o_n, _t_n) = &collected_matches[r_idx];
                    if let Some(o_n) = o_n {
                        let op = text[o_n.start_byte()..o_n.end_byte()].to_string();
                        if !seen_root_op_names.contains(&op) {
                            unique_ops_in_block.push(r_idx);
                        } else {
                            root_ops_to_remove.insert(r_idx);
                        }
                    }
                }

                if unique_ops_in_block.is_empty() && !block.3.is_empty() {
                    // Entire schema block is redundant
                    modifications.push((
                        container.start_byte(),
                        container.end_byte(),
                        "".to_string(),
                    ));
                } else if seen_schema {
                    // Convert to extend schema
                    modifications.push((
                        container.start_byte(),
                        container.start_byte() + 6,
                        "extend schema".to_string(),
                    ));
                    for &r_idx in &unique_ops_in_block {
                        let (_, _, _, _, o_n, _t_n) = &collected_matches[r_idx];
                        let op =
                            text[o_n.unwrap().start_byte()..o_n.unwrap().end_byte()].to_string();
                        seen_root_op_names.insert(op);
                    }
                } else {
                    seen_schema = true;
                    for &r_idx in &unique_ops_in_block {
                        let (_, _, _, _, o_n, _t_n) = &collected_matches[r_idx];
                        let op =
                            text[o_n.unwrap().start_byte()..o_n.unwrap().end_byte()].to_string();
                        seen_root_op_names.insert(op);
                    }
                }
                handled_schema_blocks.insert(m_idx);
            } else if *is_root_op {
                if root_ops_to_remove.contains(&m_idx) {
                    modifications.push((
                        container.start_byte(),
                        container.end_byte(),
                        "".to_string(),
                    ));
                }
            } else if let Some(name_node) = name_node {
                let name = &text[name_node.start_byte()..name_node.end_byte()];
                let is_extension = container.kind() == "type_extension";

                if !is_extension {
                    if seen_base.contains(name) {
                        let is_scalar = container.kind() == "scalar_type_definition";
                        let mut has_directives = false;
                        let mut cursor = container.walk();
                        for child in container.children(&mut cursor) {
                            if child.kind() == "directives" {
                                has_directives = true;
                                break;
                            }
                        }

                        if is_scalar && !has_directives {
                            modifications.push((
                                container.start_byte(),
                                container.end_byte(),
                                "".to_string(),
                            ));
                        } else {
                            let mut insert_pos = container.start_byte();
                            let mut cursor = container.walk();
                            for child in container.children(&mut cursor) {
                                let kind = child.kind();
                                if kind != "description" && kind != "comment" {
                                    insert_pos = child.start_byte();
                                    break;
                                }
                            }
                            modifications.push((
                                container.start_byte(),
                                insert_pos,
                                "extend ".to_string(),
                            ));
                        }
                    } else {
                        seen_base.insert(name.to_string());
                    }
                }
            }
        }

        modifications.sort_by_key(|m| m.0);
        let mut current_pos = 0;
        for (start, end, replacement) in modifications {
            if start < current_pos {
                continue;
            }
            merged.push_str(&text[current_pos..start]);
            merged.push_str(&replacement);
            current_pos = end;
        }
        merged.push_str(&text[current_pos..]);
        merged.push('\n');
    }

    merged
}

/// Simple interpolation masker for template strings.
/// Replaces ${...} with spaces of the same length to preserve offsets.
pub fn mask_interpolations(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains("${") {
        return std::borrow::Cow::Borrowed(text);
    }

    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            result.push_str("  ");
            chars.next(); // consume '{'
            let mut depth = 1;
            while depth > 0 {
                if let Some(inner_c) = chars.next() {
                    match inner_c {
                        '{' => {
                            depth += 1;
                            result.push(' ');
                        }
                        '}' => {
                            depth -= 1;
                            result.push(' ');
                        }
                        '\n' => result.push('\n'),
                        _ => {
                            for _ in 0..inner_c.len_utf8() {
                                result.push(' ');
                            }
                        }
                    }
                } else {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    std::borrow::Cow::Owned(result)
}

/// Finds the range of an operation definition by name
pub fn find_operation_range(doc: &DocumentState, operation_name: &str) -> Option<Range> {
    for block in doc.get_graphql_trees() {
        let query = GQL_SYMBOL_QUERY_CACHE.get_or_init(|| {
            let lang = tree_sitter_graphql::LANGUAGE.into();
            tree_sitter::Query::new(&lang, GQL_SYMBOL_QUERY).unwrap()
        });

        let mut cursor = tree_sitter::QueryCursor::new();
        let mut matches =
            cursor.matches(query, block.tree.root_node(), |node: tree_sitter::Node| {
                doc.rope
                    .byte_slice(
                        (node.start_byte() + block.offset)..(node.end_byte() + block.offset),
                    )
                    .chunks()
            });

        while let Some(m) = matches.next() {
            let mut name = None;
            let mut op_node = None;

            for cap in m.captures {
                let cap_name = query.capture_names()[cap.index as usize];
                if cap_name == "symbol.name" {
                    name = Some(doc.get_node_text(cap.node, block.offset));
                    op_node = Some(cap.node);
                }
            }

            if let (Some(n), Some(node)) = (name, op_node)
                && n == operation_name
            {
                return Some(doc.translate_to_file_range(node, block.offset));
            }
        }
    }

    None
}

pub fn push_duplicate_operation_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    range: Range,
    name: &str,
    other_files: Option<Vec<String>>,
) {
    let message = if let Some(files) = other_files {
        format!(
            "Duplicate operation name '{}' (also in: {})",
            name,
            files.join(", ")
        )
    } else {
        format!("Duplicate operation name '{}'", name)
    };

    diagnostics.push(Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        message,
        code: Some(NumberOrString::String("duplicate_operation".to_string())),
        source: DIAGNOSTIC_SOURCE.map(String::from),
        ..Default::default()
    });
}

pub fn to_posix_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    if cfg!(windows) {
        normalize_windows_path(&s).replace('\\', "/")
    } else {
        s.into_owned()
    }
}

pub fn normalize_uri(uri: Uri) -> Uri {
    if let Some(path) = crate::utils::uri_to_path(&uri) {
        let path = canonicalize_cached(&path);
        let path_str = path.to_string_lossy();

        #[cfg(windows)]
        let path_str = normalize_windows_path(&path_str);

        return Uri::from_file_path(Path::new(&*path_str)).unwrap_or(uri);
    }
    uri
}

pub fn normalize_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n")
}

pub fn normalize_windows_path(s: &str) -> String {
    let s = if let Some(stripped) = s.strip_prefix(r"\\?\UNC\") {
        format!("\\\\{}", stripped)
    } else if let Some(stripped) = s.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else {
        s.to_string()
    };

    // Replace all forward slashes with backslashes for consistency before processing
    let mut s = s.replace('/', "\\");

    // Handle drive letter casing: "c:\" -> "C:\"
    if s.len() >= 2 && s.as_bytes()[1] == b':' {
        let drive = s.as_bytes()[0] as char;
        if drive.is_ascii_lowercase() {
            s = format!("{}{}", drive.to_ascii_uppercase(), &s[1..]);
        }
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use globset::{Glob, GlobSetBuilder};

    #[test]
    fn test_has_generated_header_requires_a_header() {
        // What graphox itself emits.
        assert!(has_generated_header(
            "/* tslint:disable */\n/* eslint-disable */\n// This file was automatically generated and should not be edited.\n\nexport {};\n"
        ));
        assert!(has_generated_header("// @generated\nexport {};\n"));

        // A hand-written file that merely mentions the sentence is NOT generated —
        // classifying it as such drops it from the scan, and pruning would then delete
        // its live generated output.
        assert!(!has_generated_header(
            "import { gql } from \"graphql-tag\";\n// note: files under gen/ say \"This file was automatically generated and should not be edited.\"\nexport const q = gql(`query A { me }`);\n"
        ));
        assert!(!has_generated_header(
            "const banner = `This file was automatically generated and should not be edited.`;\n"
        ));
        // Too deep into the file to be a header.
        let deep = format!(
            "{}// This file was automatically generated and should not be edited.\n",
            "// filler\n".repeat(GENERATED_HEADER_MAX_LINES)
        );
        assert!(!has_generated_header(&deep));
    }

    #[test]
    #[ntest::timeout(10000)]
    fn test_get_gitignore_matcher_prunes_ignored_trees_but_honours_nested_rules() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        // `git_ignore(true)` only takes effect inside a recognised git repo.
        std::fs::create_dir_all(base.join(".git")).unwrap();
        std::fs::write(base.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();

        // Root ignore prunes node_modules; a nested ignore in a real source dir
        // adds a rule that must still be picked up.
        std::fs::write(base.join(".gitignore"), "node_modules\n").unwrap();
        std::fs::create_dir_all(base.join("src")).unwrap();
        std::fs::write(base.join("src/.gitignore"), "generated.graphql\n").unwrap();

        // A .gitignore *inside* an ignored tree must not change verdicts for real
        // files (the parent rule already ignores everything under it).
        std::fs::create_dir_all(base.join("node_modules/dep")).unwrap();
        std::fs::write(base.join("node_modules/dep/.gitignore"), "src\n").unwrap();

        let matcher = get_gitignore_matcher(base);

        // Root rule still applies (the matcher matches the ignored directory itself).
        assert!(is_path_ignored(&base.join("node_modules"), &matcher));
        // Nested rule in a non-ignored tree is honoured — this is the rule that
        // pruning the walk must not drop.
        assert!(is_path_ignored(
            &base.join("src/generated.graphql"),
            &matcher
        ));
        // Regular source files are not ignored.
        assert!(!is_path_ignored(&base.join("src/query.graphql"), &matcher));
    }

    #[test]
    #[ntest::timeout(3000)]
    fn test_merge_schema_texts_multiple_schema_blocks() {
        let schema1 = "schema { query: Query } type Query { foo: String }".to_string();
        let schema2 = "schema { query: Query } type Query { bar: String }".to_string();
        let merged = merge_schema_texts(&[schema1, schema2]);
        assert!(merged.contains("schema { query: Query }"));
        assert!(!merged.contains("extend schema { query: Query }")); // should be removed as redundant
        assert!(merged.contains("type Query { foo: String }"));
        assert!(merged.contains("extend type Query { bar: String }"));
    }

    #[test]
    #[ntest::timeout(3000)]
    fn test_merge_schema_texts_multiple_query() {
        let schema1 = "type Query { foo: String }".to_string();
        let schema2 = "type Query { bar: String }".to_string();
        let merged = merge_schema_texts(&[schema1, schema2]);
        assert!(merged.contains("type Query { foo: String }"));
        assert!(merged.contains("extend type Query { bar: String }"));

        // Verify with apollo-compiler
        let s = apollo_compiler::Schema::parse(&merged, "merged.graphql").expect("Parsing failed");
        s.validate().expect("Validation failed");
    }

    #[test]
    #[ntest::timeout(3000)]
    fn test_merge_schema_texts_conflicting_root_names() {
        let schema1 = "schema { query: Q1 } type Q1 { f1: String }".to_string();
        let schema2 = "schema { query: Q2 } type Q2 { f2: String }".to_string();
        let merged = merge_schema_texts(&[schema1, schema2]);

        println!("Merged:\n{}", merged);
        assert!(merged.contains("schema { query: Q1 }"));
        assert!(!merged.contains("extend schema { query: Q2 }"));
        assert!(!merged.contains("schema { query: Q2 }"));
        assert!(merged.contains("type Q2 { f2: String }"));

        let s = apollo_compiler::Schema::parse(&merged, "merged.graphql").expect("Parsing failed");
        s.validate().expect("Validation failed after fix");
    }

    #[test]
    #[ntest::timeout(300)]
    fn test_mask_interpolations() {
        let input = "query { user(id: ${userId}) { name } }";
        let masked = mask_interpolations(input);
        assert_eq!(masked.len(), input.len());
        assert!(masked.contains("user(id: "));
        assert!(masked.contains(") { name }"));

        let nested = "query { user(id: ${getId({a: 1})}) { name } }";
        let masked_nested = mask_interpolations(nested);
        assert_eq!(masked_nested.len(), nested.len());

        let multi_line = "query {\n  ${fragment}\n  user { id }\n}";
        let masked_multi_line = mask_interpolations(multi_line);
        assert_eq!(masked_multi_line.len(), multi_line.len());
        assert_eq!(
            masked_multi_line.lines().count(),
            multi_line.lines().count()
        );
    }

    #[test]
    fn test_mask_interpolations_multi_byte() {
        let input = "query { user(id: ${'😀'}) { name } }";
        let masked = mask_interpolations(input);
        assert_eq!(
            masked.len(),
            input.len(),
            "Byte length must be preserved even for multi-byte characters"
        );
    }

    #[test]
    #[ntest::timeout(3000)]
    fn test_get_glob_root() {
        assert_eq!(get_glob_root("src/*.ts"), PathBuf::from("src"));
        assert_eq!(
            get_glob_root("src/components/**/*.tsx"),
            PathBuf::from("src/components")
        );
        assert_eq!(get_glob_root("*.graphql"), PathBuf::from(""));
        assert_eq!(get_glob_root("docs/"), PathBuf::from("docs/"));
    }

    #[test]
    fn test_should_skip_project_walk_dir_for_irrelevant_dirs() {
        let exclude_set = GlobSetBuilder::new().build().unwrap();
        let base_dir = Path::new("/repo");

        assert!(should_skip_project_walk_dir(
            Path::new("/repo/node_modules/pkg"),
            base_dir,
            &exclude_set
        ));
        assert!(should_skip_project_walk_dir(
            Path::new("/repo/.git/objects"),
            base_dir,
            &exclude_set
        ));
        assert!(!should_skip_project_walk_dir(
            Path::new("/repo/src"),
            base_dir,
            &exclude_set
        ));
    }

    #[test]
    fn test_should_skip_project_walk_dir_for_excluded_paths() {
        let mut builder = GlobSetBuilder::new();
        builder.add(Glob::new("dist/**").unwrap());
        let exclude_set = builder.build().unwrap();
        let base_dir = Path::new("/repo");

        assert!(should_skip_project_walk_dir(
            Path::new("/repo/dist/assets"),
            base_dir,
            &exclude_set
        ));
        assert!(!should_skip_project_walk_dir(
            Path::new("/repo/src/components"),
            base_dir,
            &exclude_set
        ));
    }

    #[test]
    fn test_path_starts_with() {
        let path = Path::new("/Users/foo/bar/baz.ts");
        let prefix = Path::new("/Users/foo");
        assert!(path_starts_with(path, prefix));

        let path = Path::new("/private/Users/foo/bar/baz.ts");
        let prefix = Path::new("/Users/foo");
        #[cfg(target_os = "macos")]
        assert!(path_starts_with(path, prefix));
        #[cfg(not(target_os = "macos"))]
        assert!(!path_starts_with(path, prefix));

        let path = Path::new("/Users/foo/bar/baz.ts");
        let prefix = Path::new("/private/Users/foo");
        // The /private normalization is macOS-only; elsewhere this is a plain
        // prefix comparison and the paths genuinely do not match.
        #[cfg(target_os = "macos")]
        assert!(path_starts_with(path, prefix));
        #[cfg(not(target_os = "macos"))]
        assert!(!path_starts_with(path, prefix));

        let path = Path::new("/Users/foo/bar/baz.ts");
        let prefix = Path::new("/Users/fo");
        assert!(!path_starts_with(path, prefix));
    }
}
