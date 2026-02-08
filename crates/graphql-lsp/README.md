# graphql-lsp

The Language Server Protocol (LSP) implementation for `graphql-rust`.

## Architecture

Built on top of `tower-lsp`, this crate implements the `LanguageServer` trait and integrates the features provided by `graphql-features`.

- **Backend**: The main state holder that manages documents, schemas, and indices.
- **FileWatchers**: Monitors the workspace for changes and triggers re-validation or schema reloads.
- **Diagnostics**: Push-based diagnostics that provide real-time feedback in the editor.

## Integration

This crate is the entry point for editor integrations (VSCode, Neovim, etc.). It is started using the `lsp` subcommand of the main CLI.

