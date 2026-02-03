# Agent Instructions for graphql-rust

You are an agentic coding assistant working on `graphql-rust`, a comprehensive Rust toolset for GraphQL development. It provides a Language Server (LSP), TypeScript type generation (codegen), and validation utilities.

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

The core logic relies on **Tree-sitter** for incremental parsing and **apollo-compiler** for GraphQL schema validation and semantic analysis.

## Code Style & Conventions

### Language & Tooling
- **Rust Edition:** 2024.
- **Async Runtime:** `tokio` (multi-threaded).
- **LSP Framework:** `tower-lsp`.
- **Concurrency:** Uses `DashMap` for shared state and `Arc` for immutable data. Use `rayon` for parallel processing of files during codegen/scan.

### Formatting & Naming
- Follow standard Rust naming conventions: `PascalCase` for types/traits, `snake_case` for functions, variables, and modules.
- Use `cargo fmt` for formatting.
- Prefer explicit imports. Group imports: `std` first, then external crates, then `crate::...`.

### Error Handling
- Use `Result` and `Option` extensively.
- Avoid `unwrap()` or `expect()` in library code (`src/`) unless it's a proven invariant.
- In tests, `expect()` is acceptable for setup logic.

## Code Structure

- `src/main.rs`: CLI entry point using `clap`.
- `src/lib.rs`: Library exports and module definitions.
- `src/backend.rs`: Core LSP implementation. Manages `DashMap<Url, DocumentState>` and `DashMap<String, Arc<Schema>>`.
- `src/document.rs`: `DocumentState` manages a file's content (via `ropey`), its primary Tree-sitter tree, and any embedded GraphQL blocks.
- `src/engine.rs`: High-level operations like workspace scanning, fragment resolution, and validation.
- `src/queries.rs`: Contains Tree-sitter query strings and cached `Query` objects.
- `src/commands/`: CLI subcommand implementations (lsp, check, codegen, benchmark).
- `src/features/`: LSP feature implementations (hover, completion, definition, etc.).

## Key Patterns

### Document Management
`DocumentState` is the source of truth for a file. For TS/TSX files, it extracts GraphQL blocks by searching for template literals. These blocks are tracked with their offsets to allow mapping positions between the host language and GraphQL.
- `get_semantic_diagnostics`: Main entry point for validation.
- `apply_change`: Handles incremental updates from the LSP.

### Concurrency in LSP
The `Backend` struct is wrapped in `Arc` and shared across LSP requests. Use `DashMap` for thread-safe access to documents and schemas. Avoid holding `DashMap` write locks across `await` points.

### Tree-Sitter Queries
Queries are defined as constants in `src/queries.rs` and lazily initialized in `OnceLock`. If adding a new query:
1. Define the S-expression string.
2. Add a `OnceLock<Query>` for it.
3. Use it in `document.rs` or features via `TS_QUERY_CACHE.get_or_init(...)`.

### Codegen & Baselines
The codegen command generates TypeScript types. Tests for codegen compare output against files in `tests/baselines/`. If you intentionally change codegen output, run `make update-baselines` to update these files.

### Configuration Handling
The `Config` struct (in `src/config.rs`) defines how the tool scans the workspace. It supports multiple projects with different schemas.
- `projects`: List of project configurations.
- `base_dir`: Root directory for relative paths.
- `GlobPattern`: Used for `include` and `exclude` fields. Supports both single strings and arrays of strings. Matches are relative to `base_dir`.
- `exclude`: Optional glob patterns to exclude files from a project.

## Testing Strategy

- **Unit Tests:** Located in `src/` modules.
- **Integration Tests:** Located in `tests/`. Use `tests/fixtures/simple_schema.graphql` for most tests.
- **LSP Tests:** Use `tower-lsp`'s testing utilities to simulate client requests.
- **Fixture Based:** Add new GraphQL or TSX files to `tests/fixtures/` and use them in integration tests.

## Adding New Features

1.  **LSP Feature:**
    - Implement the logic in `src/features/`.
    - Add the method to `Backend` in `src/backend.rs` (implementing the `LanguageServer` trait).
    - Add a test case in `tests/`.
2.  **CLI Command:**
    - Add the variant to `Commands` enum in `src/main.rs`.
    - Create a new module in `src/commands/`.
    - Implement the logic, usually leveraging `Engine`.
3.  **Grammar Changes:**
    - The project uses `tree-sitter-graphql`, `tree-sitter-typescript`, and `tree-sitter-tsx`. If parsing fails, check if the query in `src/queries.rs` needs updating.

## Performance Tips

- Use `rayon`'s `par_iter()` when scanning many files.
- Minimize `Rope` to `String` conversions.
- Use `fnv::FnvHashMap` for better performance with small keys (like fragment names).
- Cache expensive computations (like schema parsing) in `Arc` or `DashMap`.

## Useful Crates & Why
- `apollo-compiler`: Industrial-strength GraphQL compiler for validation and analysis.
- `tree-sitter`: Incremental parsing, essential for real-time LSP feedback.
- `ropey`: Efficient text manipulation, used for handling LSP edits.
- `dashmap`: High-performance concurrent hash map.
- `rayon`: Simple and powerful data parallelism.
