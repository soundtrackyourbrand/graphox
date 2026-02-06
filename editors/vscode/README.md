# GraphQL Rust VSCode Extension

VSCode extension for the GraphQL Rust Language Server.

## Features

- Real-time GraphQL validation with granular diagnostics
- Autocomplete, go-to-definition, hover documentation, and find references
- Fragment dependency tracking and validation
- Semantic syntax highlighting and call hierarchy
- Automatic codegen on file changes
- Commands:
  - `GraphQL: Run Codegen` - Manually trigger type generation
  - `GraphQL: Clear Cache` - Clear the schema cache

## Development

### Prerequisites

- Node.js (v18+)
- pnpm
- Rust toolchain (to build the language server binary)

### Building and Installing Locally

1. **Build the language server binary:**

```bash
# From the project root
cargo build --release
```

2. **Install dependencies:**

```bash
# From the editors/vscode directory
pnpm install
```

3. **Compile the extension:**

```bash
pnpm run compile
```

4. **Package the extension:**

```bash
pnpm run package
```

This creates a `.vsix` file (e.g., `graphql-rust-0.1.0.vsix`) that you can install.

5. **Install the extension in VSCode:**

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
cargo build
```

3. **VSCode - Debug the extension:**
   - Open the `editors/vscode` folder in VSCode
   - Press `F5` to launch the Extension Development Host
   - This opens a new VSCode window with the extension loaded
   - Make changes and reload the window to see updates

### Configuration

The extension looks for the `graphql-rust` binary in `../../target/debug/graphql-rust` relative to the extension directory during development. For production builds, the binary should be bundled or made available in the system PATH.

## Publishing

The extension is automatically built and packaged as part of the release workflow when a version tag is pushed. See the main project README for release instructions.
