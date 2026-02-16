# Editor Setup

Graphox provides a Language Server (LSP) that can be used with various editors to provide real-time validation, autocomplete, and more.

## Supported Editors

| Editor | Setup Guide |
|--------|-------------|
| VSCode | [vscode/README.md](./vscode/README.md) |
| Neovim | [neovim.md](./neovim.md) |
| IntelliJ | [intellij.md](./intellij.md) |

## General LSP Configuration

If your editor is not listed above, you can manually configure it to use the Graphox binary as an LSP server.

**Command:** `graphox lsp`
**Filetypes:** `graphql`, `typescript`, `typescriptreact`, `javascript`, `javascriptreact`

For more information on CLI commands, see the [main README](../README.md#commands).
