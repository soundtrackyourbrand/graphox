# Graphox VSCode Extension

VSCode extension for the Graphox Language Server.

## Features

- Real-time GraphQL validation with granular diagnostics
- Autocomplete, go-to-definition, hover documentation, and find references
- Fragment dependency tracking and validation
- Semantic syntax highlighting and call hierarchy
- Automatic codegen on file changes
- Commands:
  - `GraphQL: Run Codegen` - Manually trigger type generation
  - `GraphQL: Clear Cache` - Clear the schema cache
  - `GraphQL: Restart Server` - Restart the LSP server

## Supported File Types

- `.graphql` files
- `.ts`, `.tsx` files with embedded GraphQL
- `.js`, `.jsx` files with embedded GraphQL

## Configuration

The extension supports the following settings (File > Preferences > Settings > Extensions > Graphox):

| Setting | Description | Default |
|---------|-------------|---------|
| `graphox.serverPath` | Path to Graphox binary. If empty, uses `Graphox` from PATH and falls back to `target/debug` or `target/release`. | `""` |
| `graphox.logLevel` | Logging level for the LSP server (`error`, `warn`, `info`, `debug`, `trace`). | `"info"` |

### Setting the Binary Path

If the extension cannot find the graphox binary automatically, you can manually set the path:

1. Open VSCode Settings (Cmd+, on macOS or Ctrl+, on Windows/Linux)
2. Search for "Graphox"
3. Enter the full path to the `Graphox` binary in the `Server Path` setting

For local development, you can point to:
- Debug build: `/path/to/graphox/target/debug/graphox`
- Release build: `/path/to/graphox/target/release/graphox`

## Development

### Prerequisites

- Node.js (v18+)
- pnpm
- Rust toolchain (to build the language server binary)

### Building and Installing Locally

#### Option 1: Using Release Build (Recommended)

```bash
# From the project root
cargo build --release

# From the editors/vscode directory
pnpm install
pnpm run compile
```

#### Option 2: Using Debug Build

```bash
# From the project root
cargo build

# From the editors/vscode directory
pnpm install
pnpm run compile
pnpm run build:dev  # Alias for cargo build
```

### Packaging the Extension

```bash
pnpm run package
```

This creates a `.vsix` file (e.g., `graphox-0.1.0.vsix`) that you can install.

### Installing the Extension

```bash
pnpm run install-local
```

Or manually install via VSCode:
- Open VSCode
- Press `Cmd+Shift+P` (Mac) or `Ctrl+Shift+P` (Windows/Linux)
- Type "Install from VSIX"
- Select the generated `.vsix` file

### Development Workflow

For active development with live reloading:

1. **Terminal 1 - Watch the TypeScript compilation:**

```bash
pnpm run watch
```

2. **Terminal 2 - Rebuild the Rust binary when needed:**

```bash
# From project root
cargo build  # or cargo build --release
```

3. **Terminal 3 - Restart the server after Rust changes:**

When you make changes to the Rust code, rebuild the binary and run the `GraphQL: Restart Server` command in VSCode (Cmd+Shift+P > "GraphQL: Restart Server").

4. **VSCode - Debug the extension:**
   - Open the `editors/vscode` folder in VSCode
   - Press `F5` to launch the Extension Development Host
   - This opens a new VSCode window with the extension loaded
   - Make changes and reload the window to see updates

### Building the Rust Binary

The extension can use either debug or release builds:

```bash
# Debug build (faster to compile, slower to run)
cargo build
pnpm run build:dev  # Also rebuilds Rust binary

# Release build (slower to compile, faster to run)
cargo build --release
pnpm run build:release
```

The extension looks for binaries in this order:
1. Custom path set in `graphox.serverPath` setting
2. `Graphox` command in system PATH
3. `target/release/graphox` relative to repository root
4. `target/debug/graphox` relative to repository root

## Publishing

The extension is automatically built and packaged as part of the release workflow when a version tag is pushed. See the main project README for release instructions.

To publish manually:

```bash
# Update version in package.json
# Build the extension
pnpm run package

# Publish to Open VSX (free)
npx vsce publish --packagePath graphox-*.vsix

# Or publish to VS Code Marketplace (requires publisher account)
npx vsce publish
```

## Troubleshooting

### Binary Not Found

If you see an error about the binary not being found:

1. Make sure the Rust binary is built: `cargo build --release`
2. Set the `graphox.serverPath` setting to the full path of the binary
3. Try restarting the server: Cmd+Shift+P > "GraphQL: Restart Server"

### Server Not Starting

Check the Output panel in VSCode:
1. Open View > Output
2. Select "Graphox Language Server" from the dropdown
3. Look for error messages

You can also increase the log level to `debug` or `trace` in the settings for more verbose output.
