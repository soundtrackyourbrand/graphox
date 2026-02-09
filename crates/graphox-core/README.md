# graphox-core

The foundation of the `Graphox` toolset. This crate provides the core models, schema management, and validation engine used by all other crates in the workspace.

## Key Components

- **DocumentState**: Manages the lifecycle of a single GraphQL or host (TS/TSX) file, including Tree-sitter parsing and embedded block extraction.
- **Engine**: Orchestrates workspace-wide operations like scanning for fragments and operations.
- **Schema & SchemaCache**: Handles GraphQL schema loading, merging, and two-tier (memory + disk) caching for performance.
- **Config**: Parses and validates the `graphox.yaml` configuration.

## Usage

This crate is intended for internal use by other `Graphox` crates, but it can be used independently for tools that need to parse and analyze GraphQL in a workspace.

