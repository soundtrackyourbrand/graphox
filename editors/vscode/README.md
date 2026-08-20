# Graphox VS Code Extension

Graphox is a high-performance GraphQL toolkit built in Rust, including a Language Server and TypeScript code generation.  
This extension brings Graphox into VS Code for fast validation, navigation, and schema-aware editing.

## What you get

- Real-time GraphQL diagnostics for standalone and embedded GraphQL
- Autocomplete, hover, go-to-definition, and find references
- Fragment-aware validation across your workspace
- Commands:
  - `Graphox: Run Codegen`
  - `Graphox: Clear Cache`
  - `Graphox: Restart Server`

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
| `graphox.rustBacktrace` | Value passed as `RUST_BACKTRACE` when launching the server. Set to `1` or `full` for crash backtraces. Leave empty to keep it unset. | `""` |
| `graphox.verboseWatcherLogging` | Log every watched file event forwarded to Graphox, including URI and event type, in the Graphox output channel. Useful for debugging watcher storms. | `false` |

Graphox activates automatically when your workspace root contains `graphox.yaml` or `graphox.yml`.

## Troubleshooting

If the server does not start, open **View > Output** and select **Graphox Language Server** to inspect startup logs.

If the server crashes and you need a Rust backtrace, set `graphox.rustBacktrace` to `1` or `full`, restart the server, and reproduce the issue. The chosen value is echoed in the Graphox output channel when the server starts.

If you need to debug excessive file watcher traffic, enable `graphox.verboseWatcherLogging` and watch the **Graphox** output channel. Each forwarded file event is logged as `[watcher] <event-type> <file-uri>`.

## Resources

- Repository: https://github.com/soundtrackyourbrand/graphox

## License

MIT — see [LICENSE](./LICENSE).
