//! The disposable clone the scenario runs in, git operations on it, and the
//! files the scenario edits.
//!
//! Targets are picked from the repository itself, using graphox's own config
//! and parser, so the same scenario runs against any graphox workspace.

use graphox_core::{Config, DocumentState};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

pub type Result<T> = std::result::Result<T, String>;

pub fn git(repo: &Path, args: &[&str]) -> Result<String> {
    // The scenario's commits and checkouts are throwaway, so they run without
    // hooks and signing, under a fixed identity.
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "advice.detachedHead=false",
        ])
        .args(args)
        .env("GIT_AUTHOR_NAME", "graphox-lsp-scenario")
        .env("GIT_AUTHOR_EMAIL", "scenario@graphox.invalid")
        .env("GIT_COMMITTER_NAME", "graphox-lsp-scenario")
        .env("GIT_COMMITTER_EMAIL", "scenario@graphox.invalid")
        .output()
        .map_err(|e| format!("git {}: {e}", args.join(" ")))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Clones `source` into `work/repo` (or refreshes an earlier clone) and resets
/// it to `rev` with no untracked or ignored files, so every run starts from
/// the same state: no codegen output on disk.
pub fn prepare(source: &Path, work: &Path, rev: &str) -> Result<(PathBuf, String)> {
    let base_sha = git(source, &["rev-parse", rev])?.trim().to_string();
    std::fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let work = work.canonicalize().map_err(|e| e.to_string())?;
    let repo = work.join("repo");
    if !repo.join(".git").exists() {
        let source = source.to_string_lossy();
        let repo_s = repo.to_string_lossy();
        // `--local` hardlinks the object store, so a large repository costs
        // little disk and seconds of time.
        git(
            &work,
            &["clone", "--local", "--no-checkout", &source, &repo_s],
        )?;
    } else {
        git(&repo, &["fetch", "--quiet", "--prune", "origin"])?;
    }
    let _ = git(&repo, &["rebase", "--abort"]);
    git(
        &repo,
        &["checkout", "--quiet", "--force", "--detach", &base_sha],
    )?;
    git(&repo, &["clean", "-fdxq"])?;
    for branch in git(
        &repo,
        &[
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads/scenario/",
        ],
    )?
    .lines()
    {
        git(&repo, &["branch", "-D", branch])?;
    }
    let _ = git(&repo, &["stash", "clear"]);
    let repo = repo.canonicalize().map_err(|e| e.to_string())?;
    Ok((repo, base_sha))
}

/// An insertion point inside a GraphQL definition: just after its opening
/// brace, where `__typename` is valid whatever the type.
#[derive(Clone, Debug)]
pub struct Site {
    pub path: PathBuf,
    pub insert_at: usize,
    pub name: String,
    /// Byte offset of the definition's name, for rename and navigation.
    pub name_at: usize,
}

#[derive(Debug)]
pub struct Targets {
    pub operation: Site,
    pub fragment: Site,
    pub fragment_consumers: usize,
    /// Files an editor would plausibly have open.
    pub open_files: Vec<PathBuf>,
    /// Operations spread over many files, for edits made outside the editor.
    pub bulk_sites: Vec<Site>,
    pub schema_file: Option<PathBuf>,
    pub project_documents: usize,
    pub graphql_documents: usize,
}

struct Scanned {
    path: PathBuf,
    project: String,
    operations: Vec<String>,
    fragments: Vec<String>,
    spreads: BTreeSet<String>,
    text: String,
}

fn find_site(path: &Path, text: &str, pattern: &str, name: &str) -> Option<Site> {
    let re = Regex::new(pattern).ok()?;
    let m = re.captures(text)?;
    let name_match = m.name("name")?;
    Some(Site {
        path: path.to_path_buf(),
        insert_at: m.get(0)?.end(),
        name: name.to_string(),
        name_at: name_match.start(),
    })
}

fn operation_site(scanned: &Scanned, name: &str) -> Option<Site> {
    let pattern = format!(
        r"\b(?:query|mutation|subscription)\s+(?P<name>{})\b[^{{]*\{{",
        regex::escape(name)
    );
    find_site(&scanned.path, &scanned.text, &pattern, name)
}

fn fragment_site(scanned: &Scanned, name: &str) -> Option<Site> {
    let pattern = format!(
        r"\bfragment\s+(?P<name>{})\s+on\s+\w+[^{{]*\{{",
        regex::escape(name)
    );
    find_site(&scanned.path, &scanned.text, &pattern, name)
}

pub fn discover(repo: &Path, config: &Config, count_bulk: usize) -> Result<Targets> {
    let files = git(repo, &["ls-files", "-z"])?;
    let mut scanned = Vec::new();
    let mut project_documents = 0;
    for rel in files.split('\0').filter(|f| !f.is_empty()) {
        let path = repo.join(rel);
        if !graphox_core::utils::is_relevant_file(&path) || config.is_output_file(&path) {
            continue;
        }
        let Some(project) = config.get_project_for_path(&path) else {
            continue;
        };
        project_documents += 1;
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let is_graphql = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("graphql" | "gql")
        );
        if !is_graphql && !text.contains("gql") && !text.contains("graphql") {
            continue;
        }
        let Some(uri) = graphox_core::utils::path_to_uri(&path) else {
            continue;
        };
        let doc =
            DocumentState::new_from_thread_local(uri, &text, ls_types::PositionEncodingKind::UTF16);
        if doc.get_graphql_trees().is_empty() {
            continue;
        }
        scanned.push(Scanned {
            path,
            project: project.include().as_key(),
            operations: doc
                .operations()
                .iter()
                .filter_map(|o| o.name.as_deref().map(str::to_string))
                .collect(),
            fragments: doc.fragments.iter().map(|f| f.name.to_string()).collect(),
            spreads: doc.fragment_spreads.iter().map(|s| s.to_string()).collect(),
            text,
        });
    }
    scanned.sort_by(|a, b| a.path.cmp(&b.path));

    // The most-spread fragment is the one whose edits fan out the furthest.
    let mut consumers: HashMap<&str, usize> = HashMap::new();
    for s in &scanned {
        for spread in &s.spreads {
            *consumers.entry(spread.as_str()).or_default() += 1;
        }
    }
    let definitions: HashMap<&str, &Scanned> = scanned
        .iter()
        .flat_map(|s| s.fragments.iter().map(move |f| (f.as_str(), s)))
        .collect();
    let mut ranked: Vec<(&str, usize)> = consumers
        .iter()
        .filter(|(name, _)| definitions.contains_key(*name))
        .map(|(n, c)| (*n, *c))
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    let (fragment, fragment_consumers) = ranked
        .iter()
        .find_map(|(name, count)| Some((fragment_site(definitions[name], name)?, *count)))
        .ok_or("no fragment spread anywhere in the workspace")?;

    // Edit an operation in the busiest project, where per-project work is largest.
    let mut per_project: BTreeMap<&str, usize> = BTreeMap::new();
    for s in &scanned {
        *per_project.entry(s.project.as_str()).or_default() += 1;
    }
    let busiest = per_project
        .iter()
        .max_by_key(|(_, n)| **n)
        .map(|(p, _)| *p)
        .ok_or("no GraphQL documents in any project")?;
    let operation = scanned
        .iter()
        .filter(|s| s.project == busiest && s.path != fragment.path)
        .find_map(|s| s.operations.iter().find_map(|o| operation_site(s, o)))
        .ok_or("no named operation in the busiest project")?;

    let mut open_files = vec![operation.path.clone(), fragment.path.clone()];
    open_files.extend(
        scanned
            .iter()
            .filter(|s| s.spreads.contains(&fragment.name))
            .map(|s| s.path.clone())
            .filter(|p| !open_files.contains(p))
            .take(6)
            .collect::<Vec<_>>(),
    );

    // Spread bulk edits evenly across the workspace rather than one directory.
    let with_ops: Vec<&Scanned> = scanned
        .iter()
        .filter(|s| !s.operations.is_empty() && !open_files.contains(&s.path))
        .collect();
    let step = (with_ops.len() / count_bulk.max(1)).max(1);
    let bulk_sites = with_ops
        .iter()
        .step_by(step)
        .filter_map(|s| s.operations.iter().find_map(|o| operation_site(s, o)))
        .take(count_bulk)
        .collect();

    let mut schema_uses: BTreeMap<String, usize> = BTreeMap::new();
    for project in config.projects() {
        for file in project.schema().files() {
            *schema_uses.entry(file).or_default() += 1;
        }
    }
    let schema_file = schema_uses
        .iter()
        .max_by_key(|(_, n)| **n)
        .map(|(f, _)| config.base_dir().join(f))
        .filter(|p| p.is_file());

    Ok(Targets {
        operation,
        fragment,
        fragment_consumers,
        open_files,
        bulk_sites,
        schema_file,
        project_documents,
        graphql_documents: scanned.len(),
    })
}

/// Of the recently active branches, the one whose checkout rewrites the most
/// GraphQL-bearing files relative to `base`.
pub fn pick_branch(repo: &Path, base: &str, candidates: usize) -> Result<Option<(String, usize)>> {
    let refs = git(
        repo,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)",
            "refs/remotes/origin",
        ],
    )?;
    let mut best: Option<(String, usize)> = None;
    for name in refs
        .lines()
        .filter(|r| *r != "origin/HEAD" && *r != "origin" && *r != "origin/main")
        .take(candidates)
    {
        let diff = git(
            repo,
            &[
                "diff",
                "--name-only",
                base,
                name,
                "--",
                "*.ts",
                "*.tsx",
                "*.graphql",
                "*.graphqls",
            ],
        )?;
        let changed = diff.lines().count();
        if best.as_ref().is_none_or(|(_, n)| changed > *n) {
            best = Some((name.to_string(), changed));
        }
    }
    Ok(best)
}

/// Files changed between two revisions, relative to the repository root.
pub fn changed_files(repo: &Path, from: &str, to: &str) -> Result<BTreeSet<String>> {
    Ok(git(repo, &["diff", "--name-only", from, to])?
        .lines()
        .map(str::to_string)
        .collect())
}
