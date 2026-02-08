# Agent Instructions for graphql-rust

You are an agentic coding assistant working on `graphql-rust`, a comprehensive Rust toolset for GraphQL development. It provides a Language Server (LSP), TypeScript type generation (codegen), and validation utilities.

All modifying git actions MUST be handled by the user. Never commit and never even suggest committing.


## Build, Lint, and Test Commands

- **Build:** `cargo build`
- **Lint:** `cargo clippy`
- **Format:** `cargo fmt`
- **Run all tests:** `cargo test`
- **Run a specific test file:** `cargo test --test <filename>` (e.g., `cargo test --test validation`)
- **Run a single test:** `cargo test <test_name_substring>` (e.g., `cargo test test_validation_valid_query`)
- **Benchmarks:** `make benchmark` or `cargo run -- benchmark <path>`
- **Update test baselines:** `make update-baselines` (runs `./scripts/update_baselines.py`)
- **Clean:** `make clean`

## Project Overview

`graphql-rust` handles GraphQL in two main forms:
1.  **Standalone:** `.graphql` files containing schemas or operations.
2.  **Embedded:** GraphQL operations inside TypeScript/TSX template literals (e.g., `gql` or `graphql` tags).

The core logic relies on **Tree-sitter** for incremental parsing and **apollo-compiler** for GraphQL schema validation and semantic analysis. The workspace also includes plugins (e.g., `plugins/swc`) for integration with other tools.

## Code Style & Conventions

### Comments
Comments should be used to explain why something is done, not what is being done (the code should be clear enough to convey the "what"). Avoid comments that simply restate the code or add no new information.

### Language & Tooling
- **Rust Edition:** 2024.
- **Async Runtime:** `tokio` (multi-threaded).
- **LSP Framework:** `tower-lsp`.
- **Concurrency:** Uses `DashMap` for shared state and `Arc` for immutable data. Use `rayon` for parallel processing of files during codegen/scan.
- **Performance Tracing:** Built-in tracing for LSP requests that exceed a threshold (configurable via `tracing` in `graphql.yaml`).

### Formatting & Naming
- Follow standard Rust naming conventions: `PascalCase` for types/traits, `snake_case` for functions, variables, and modules.
- Use `cargo fmt` for formatting.
- Prefer explicit imports. Group imports: `std` first, then external crates, then `crate::...`.

### Error Handling
- Use `Result` and `Option` extensively.
- Avoid `unwrap()` or `expect()` in library code (`src/`) unless it's a proven invariant.
- In tests, `expect()` is acceptable for setup logic.

## Code Structure

This project is organized as a Rust workspace to separate concerns and improve maintainability.

### Core Workspace Packages

- **`graphql-core`** (`crates/graphql-core`): The foundation of the toolset.
    - `document.rs`: `DocumentState` manages a file's content (via `ropey`), its Tree-sitter tree, and embedded GraphQL blocks.
    - `engine.rs`: High-level operations like workspace scanning, fragment resolution, and validation orchestration.
    - `schema.rs` & `schema_cache.rs`: Schema loading and two-tier caching.
    - `config.rs`: Configuration file (`graphql.yaml`) parsing.
    - `queries.rs`: Tree-sitter query management.
- **`graphql-features`** (`crates/graphql-features`): Implementation of GraphQL-specific intelligence.
    - LSP capabilities (Hover, Completion, etc.) are implemented as extension traits on `DocumentState`.
    - `diagnostics/`: Granular diagnostic rules (fragments, operations, selection sets, values).
- **`graphql-codegen`** (`crates/graphql-codegen`): Standalone crate for TypeScript type generation.
- **`graphql-lsp`** (`crates/graphql-lsp`): The Language Server implementation.
    - `backend/lsp.rs`: Main `Backend` struct and LSP protocol implementation using `tower-lsp`.
    - `backend/file_change_handler.rs`: Processes file system changes and updates state.

### CLI and Re-exports

- `src/main.rs`: Root CLI entry point. Supports `lsp`, `check`, `codegen`, and `benchmark` subcommands.
- `src/lib.rs`: Consolidates the public API by re-exporting modules from the workspace crates for backward compatibility.
- `src/commands/`: CLI subcommand implementations.

## Key Patterns

### LSP Feature Extension Traits
LSP features are decoupled from `DocumentState` using extension traits defined in `graphql-features`. To use a feature, you must import the corresponding trait:
```rust
use graphql_features::hover::DocumentHover;
let hover = document.get_hover_info(params, schema, engine);
```

### Workspace Scanning & Performance
The tool is designed for very large projects. Workspace scanning is parallelized using `rayon`.
- `Engine::scan_workspace`: Discovers all GraphQL fragments and operations across the workspace in parallel.
- **Indexing:** `Backend` maintains several indices for fast lookup:
    - `fragment_defs`: Maps URL to fragment definitions in that file.
    - `fragment_dependents`: Maps fragment name to files that use it.
    - `fragment_definitions`: Maps fragment name to files where it is defined.
- **Cancellation:** Long-running operations like workspace scans are cancellable via `AtomicBool`.

### Document Management
`DocumentState` is the source of truth for a file. For TS/TSX files, it extracts GraphQL blocks by searching for template literals. These blocks are tracked with their offsets to allow mapping positions between the host language and GraphQL.
- `get_semantic_diagnostics`: Main entry point for validation. Uses granular rules from `features/diagnostics/`.
- `apply_change`: Handles incremental updates from the LSP, ensuring the Tree-sitter tree stays in sync.

### Concurrency in LSP
The `Backend` struct is wrapped in `Arc` and shared across LSP requests. Use `DashMap` for thread-safe access to documents and schemas. Avoid holding `DashMap` write locks across `await` points.

### Tree-Sitter Queries
Queries are defined as constants in `src/queries.rs` and lazily initialized in `OnceLock`. If adding a new query:
1. Define the S-expression string.
2. Add a `OnceLock<Query>` for it.
3. Use it in `document.rs` or features via `TS_QUERY_CACHE.get_or_init(...)`.

### Schema Caching
The schema cache (`src/schema_cache.rs`) provides two-tier caching for performance:
- **Memory cache (L1):** Holds fully parsed and validated `Schema` objects. Fastest, no I/O. Lifetime is process duration. Invalidation checks file mtimes.
- **Disk cache (L2):** Holds merged schema text in OS-specific cache directory. Skips file I/O and merging but still requires parsing. Persistent across runs.
- Cache keys are based on schema source paths and file modification times.
- Use `load_schema_with_cache()` to leverage caching (95-99% faster for L1, 10-80% faster for L2).
- Disable caching in `graphql.yaml` with `enable_schema_cache: false` if needed.

### Codegen & Baselines
The codegen command generates TypeScript types. Tests for codegen MUST use the fixtures and baselines structure. Place input GraphQL/TS files in `tests/fixtures/` and compare generated output against files in `tests/baselines/`.
- **Entrypoint:** A `graphql.ts` file is generated in the root of the `output_dir` providing a type-safe `graphql` function.
- **Incremental Codegen:** The LSP can automatically run codegen on file changes if `lsp_automatic_codegen` is enabled.
- **Throttling:** Automatic LSP codegen is throttled (default: 300ms) to prevent storms when many files change. The `codegen --watch` command uses debouncing (default: 200ms) for similar protection.

### Configuration Handling
The `Config` struct (in `src/config.rs`) supports complex workspace setups.
- `projects`: List of project configurations with their own schemas and include/exclude patterns.
- `schema_types`: Configuration for generating global schema types.
- `scalars`: Mapping of GraphQL scalars to TypeScript types.
- `tracing`: Configuration for performance tracing.
- `ignore_deprecations`: List of deprecated fields/types to ignore in validation.
- `lsp_codegen_throttle_ms`: Throttle delay for automatic LSP codegen (default: 300ms).
- `codegen_watch_debounce_ms`: Debounce delay for watch mode file changes (default: 200ms).

**Creating Test Configs:** Use the `Default` trait with struct update syntax to make tests resilient to config changes:
```rust
let config = Config {
    base_dir: test_dir.to_path_buf(),
    projects: vec![...],
    lsp_automatic_codegen: Some(false), // Only set fields you need
    ..Default::default() // All other fields default to None
};
```
This pattern prevents tests from breaking when new optional config fields are added.

## Testing Strategy

- **Test Coverage:** High test coverage is mandatory. Every new feature or bug fix must include corresponding tests.
- **Integration Tests:** Use `tests/fixtures/` for realistic scenarios. Integration tests should cover LSP interactions, CLI commands, and complex fragment resolution.
- **Codegen Baselines:** Always verify codegen output against baselines. If changes are expected, run `make update-baselines`.
- **Performance Benchmarks:** Performance is a first-class citizen. Use `make benchmark` and `criterion` benchmarks to ensure no regressions, especially for large schemas and many-file workspaces.
- **LSP Reliability:** Use `tower-lsp`'s testing utilities. Avoid `sleep` in tests; use proper synchronization or wait for specific states.

## Adding New Features

1.  **LSP Feature:**
    - Implement logic in `src/features/`.
    - Add the method to `Backend` in `src/backend.rs`.
    - Add integration tests in `tests/`.
2.  **CLI Command:**
    - Add to `Commands` enum in `src/main.rs` and implement in `src/commands/`.
3.  **Grammar/Query Changes:**
    - Update `src/queries.rs` if needed. Verify against multiple host languages (TS, TSX, GraphQL).

## Performance Tips

- **Parallelism:** Use `rayon` for data-heavy tasks (scanning, codegen, workspace-wide validation).
- **Caching:** Cache parsed schemas (`Arc<Valid<Schema>>`) and Tree-sitter queries.
- **Granular Validation:** Only re-validate files affected by a change. Use the `fragment_dependents` index to find affected files when a fragment changes.
- **Minimize Allocations:** Avoid frequent `Rope` to `String` conversions. Use `byte_slice` or `chunks` where possible.
- **Fast Hashing:** Use `AHashMap` or `ahash` for performance-critical maps.
