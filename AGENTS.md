# Agent Instructions for Graphox

Graphox is a Rust toolset for GraphQL: a language server, TypeScript codegen, and
validation. It handles GraphQL both in standalone `.graphql` files and embedded in
TS/TSX template literals (`gql` / `graphql` tags). Tree-sitter does the incremental
parsing; apollo-compiler does schema validation and semantic analysis.

Commit, push and open pull requests only when asked.

## Commands

- `cargo build`, `cargo test --workspace`
- `cargo test <name>` for one test, `cargo test --test <suite>` for one file
- `cargo clippy --workspace` — `--workspace` matters. Without it the trailing
  `-D warnings` only reaches the root package, and lints in `plugins/swc/rust` are
  printed but never fail the build.
- `make check` — fmt, clippy, all tests including the JS plugins, and bench
  compilation. Run it before treating a change as done.
- `make benchmark`, `make update-baselines`
- Search with `rg`, not `grep`.

## Layout

Crates under `crates/`:

- **`graphox-core`** — the foundation. `document.rs` holds `DocumentState` (a rope, its
  Tree-sitter tree, and any embedded GraphQL blocks with their offsets, so positions map
  between host language and GraphQL). `engine.rs` does workspace scanning and fragment
  resolution. Plus `schema.rs` / `schema_cache.rs`, `config.rs`, `queries.rs`.
- **`graphox-features`** — GraphQL intelligence. LSP capabilities are extension traits on
  `DocumentState`; `diagnostics/` holds the validation rules.
- **`graphox-codegen`** — TypeScript generation.
- **`graphox-lsp`** — the server. `backend/lsp.rs` has `Backend` and the protocol
  implementation, `backend/file_change_handler.rs` processes file system changes.
- **`graphox-cli`** — the `check`, `codegen` (with watch mode) and `benchmark` commands.

`src/main.rs` is the CLI entry point; `src/lib.rs` re-exports the crates as the public
API. Build-tool plugins live in `plugins/{swc,babel}` — see their READMEs.

## Patterns

**Extension traits.** LSP features are decoupled from `DocumentState`, so using one means
importing its trait:

```rust
use graphox_features::hover::DocumentHover;
let hover = document.get_hover_info(params, schema, engine);
```

**Concurrency.** `Backend` is `Arc`-shared across requests, with `DashMap` for documents
and schemas. Never hold a `DashMap` write lock across an `await`. Workspace scans run on
`rayon` and are cancellable through an `AtomicBool`.

**Incremental work.** `apply_change` keeps the Tree-sitter tree in sync with LSP edits.
Re-validate only what a change affects: `fragment_dependents` maps a fragment name to the
files using it.

**Tree-sitter queries.** S-expression constants in `crates/graphox-core/src/queries.rs`,
lazily built into `OnceLock`s and reached via `get_or_init`. Verify a new one against TS,
TSX and GraphQL.

**Schema cache.** Two tiers: validated `Schema`s in memory, invalidated by mtime, and
merged schema text on disk in the OS cache directory. Go through
`load_schema_with_cache()`. `enable_schema_cache: false` turns both off.

**Configuration.** `Config` parses `graphox.yaml`; the options and their semantics are
documented in `docs/configurations.md`. Its fields are private, so build test configs with
`Config::new_test(base_dir, projects)` and the `with_*` builders.

**Performance.** It is a first-class concern here: these tools run over very large
workspaces. Prefer `AHashMap` / `ahash` for hot maps, and avoid `Rope` → `String`
conversions — use `byte_slice` or `chunks`.

## Testing

Every feature and bug fix needs tests.

Codegen is baseline-tested: inputs in `tests/fixtures/`, expected output in
`tests/baselines/`, compared by `run_baseline_test`. For a new pair, register it in
`scripts/update_baselines.py`, run `make update-baselines`, then
`python3 scripts/verify_baselines.py` to typecheck the generated TypeScript.

No `sleep` in LSP tests — synchronise on the state being waited for.

## Adding a feature

- **LSP:** logic in `graphox-features`, then wire it into `Backend` in
  `crates/graphox-lsp/src/backend/lsp.rs`.
- **CLI:** extend the `Commands` enum in `src/main.rs`, implement under
  `crates/graphox-cli/src/commands/`.

Comments should explain why, not what.
