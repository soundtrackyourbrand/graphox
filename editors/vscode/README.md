# Graphox VS Code Extension

Graphox is a high-performance GraphQL toolkit built in Rust, including a Language Server and TypeScript code generation.  
This extension brings Graphox into VS Code for fast validation, navigation, and schema-aware editing.

## What you get

- Real-time GraphQL diagnostics for standalone and embedded GraphQL
- Autocomplete, hover, go-to-definition, and find references
- Fragment-aware validation across your workspace
- Commands:
  - `GraphQL: Run Codegen`
  - `GraphQL: Clear Cache`
  - `GraphQL: Restart Server`

## Supported files

- `.graphql`
- `.ts`, `.tsx`
- `.js`, `.jsx`

## Configuration

The extension settings are available in **Settings > Extensions > Graphox**:

| Setting | Description | Default |
| --- | --- | --- |
| `graphox.serverPath` | Path to the `graphox` binary. Leave empty for automatic detection. | `""` |
| `graphox.logLevel` | LSP log level (`error`, `warn`, `info`, `debug`, `trace`). | `"info"` |

Graphox activates automatically when your workspace root contains `graphox.yaml` or `graphox.yml`.

## Troubleshooting

If the server does not start, open **View > Output** and select **Graphox Language Server** to inspect startup logs.

## Resources

- Repository: https://github.com/soundtrackyourbrand/graphox
