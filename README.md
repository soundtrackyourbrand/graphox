# graphql-rust

A high-performance GraphQL toolset for TypeScript monorepos, providing LSP, type generation, and validation.

## Features

**Language Server (LSP)**
- Real-time GraphQL validation with granular diagnostics
- Autocomplete, go-to-definition, hover documentation, and find references
- Fragment dependency tracking and validation
- Semantic syntax highlighting and call hierarchy
- File watching with incremental updates
- Automatic codegen on file changes

**Type Generation (Codegen)**
- TypeScript type generation from GraphQL operations
- Apollo AST generation for apollo-client
- Shared fragment support between packages

**Supported Formats**
- Standalone `.graphql` files
- Embedded GraphQL in TypeScript/TSX template literals (`gql`, `graphql` tags)

---

## Quick Start

1. **Install the CLI**
   ```bash
   pnpm add @soundtrack/graphql-rust-cli
   ```

2. **Create configuration**
   ```yaml
   # graphql.yaml
   output_dir: "__generated__"
   projects:
     - schema: "schema.graphql"
       include: "src/**/*.{ts,tsx}"
   ```

3. **Set up your editor** - See [Editor Setup](#editor-setup)

4. **Run commands**
   ```bash
   pnpm graphql-rust check    # Validate GraphQL files
   pnpm graphql-rust codegen   # Generate TypeScript types
   pnpm graphql-rust lsp       # Start LSP (for editors)
   ```

---

## Installation

### NPM Package (Recommended)

Install via pnpm to automatically download the correct binary for your platform:

```bash
pnpm add @soundtrack/graphql-rust-cli
npm install @soundtrack/graphql-rust-cli
yarn add @soundtrack/graphql-rust-cli
```

Then use with pnpm:

```bash
pnpm graphql-rust lsp
pnpm graphql-rust check
pnpm graphql-rust codegen
```

Or install globally:

```bash
pnpm add -g @soundtrack/graphql-rust-cli
graphql-rust lsp
graphql-rust check
graphql-rust codegen
```

**GitHub Packages:**

```bash
pnpm add @soundtrack/graphql-rust-cli --registry=https://npm.pkg.github.com
```

### Manual Binary Installation

Download pre-built binaries from the [releases page](https://github.com/soundtrack/graphql-rust/releases) for:
- macOS (x86_64, ARM64)
- Linux (x86_64, ARM64)
- Windows (x86_64, ARM64)

---

## Build Tool Plugins

Optimize bundle size by ensuring GraphQL AST files are properly codesplit.

| Build Tool | Plugin | Documentation |
|------------|--------|---------------|
| rsbuild | SWC Plugin | [plugins/swc/README.md](plugins/swc/README.md) |
| Turbopack/Next.js | SWC Plugin | [plugins/swc/README.md](plugins/swc/README.md) |
| React Native (Metro) | Babel Plugin | [plugins/babel/README.md](plugins/babel/README.md) |
| Webpack | Babel Plugin | [plugins/babel/README.md](plugins/babel/README.md) |

---

## Editor Setup

Set up `graphql-rust` as a language server in your editor:

| Editor | Setup Guide |
|--------|-------------|
| VSCode | [editors/vscode/README.md](editors/vscode/README.md) |
| Neovim | [editors/neovim.md](editors/neovim.md) |
| IntelliJ | [editors/intellij.md](editors/intellij.md) |

### Quick Editor Configuration

**VSCode:** Install the [GraphQL Rust extension](https://marketplace.visualstudio.com/items?itemName=graphql-rust.graphql-rust) or use the npm package.

**Neovim:** Configure LSP with `nvim-lspconfig`:

```lua
require('lspconfig').graphql_rust.setup({
  cmd = { 'pnpm', 'exec', 'graphql-rust', 'lsp' },
  filetypes = { 'graphql', 'typescript', 'typescriptreact' },
})
```

**IntelliJ/JetBrains:** Install LSP4IJ plugin and configure to run `pnpm exec graphql-rust lsp`.

---

## Commands

```bash
# Start the Language Server
graphql-rust lsp

# Validate GraphQL files
graphql-rust check

# Generate TypeScript types
graphql-rust codegen
graphql-rust codegen --clean  # Remove generated files and caches
graphql-rust codegen --watch   # Watches and runs codegen of file changes

# Run performance benchmarks
graphql-rust benchmark
```

### Command Options

- `check` - Validates all GraphQL files against the schema
- `codegen` - Generates TypeScript types for operations
- `lsp` - Starts the Language Server Protocol server

---

## Configuration

Create a `graphql.yaml` file in your project root:

### Basic Example

```yaml
output_dir: "__generated__"

projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
    exclude: "**/*.test.ts"
```

### Full Configuration

```yaml
# Global output directory for generated types
output_dir: "__generated__"

# Custom scalar type mappings
scalars:
  DateTime: "Date"
  JSON: "Record<string, any>"
  BigInt: "string"

# Ignore specific deprecation reasons in validation
ignore_deprecations:
  - "EXPERIMENTAL"
  - "INTERNAL"

# Generate and reuse AST nodes for fragments (for smaller bundle sizes)
generate_ast_for_fragments: false

# Project configurations (required)
projects:
  - schema: "schema.graphql"                    # Single schema file
    include: "src/client/**/*.{ts,tsx}"         # Glob pattern(s)
    exclude: "**/*.test.ts"                     # Optional exclusions
    output_dir: "src/client/__generated__"      # Override global output_dir
    import: "@workspace/project-1"              # How other project should import fragments from this project
    generate_permissions: true                  # Generate permission metadata
    codegen: true                               # Enable codegen for this project (default: true)

  - schema:                                     # Multiple schema files
      - "schema/base.graphql"
      - "schema/auth.graphql"
    include:                                    # Multiple include patterns
      - "src/server/**/*.ts"
      - "lib/**/*.graphql"
    codegen: false                              # Disable codegen for this project

# Generate standalone schema types (optional)
schema_types:
  - schema: "schema.graphql"
    output: "types/schema.ts"
    import: "@workspace/schema"                 # How other generated files should import this generation

# LSP settings
lsp_automatic_codegen: true  # Auto-run codegen on file changes (default: true)
lsp_codegen_throttle_ms: 300 # Throttle automatic codegen to prevent storms (default: 300ms)
watch_all_files: true        # Watch all workspace files (default: true)
tracing:
  enabled: true              # Enable LSP request tracing
  threshold_ms: 20           # Only trace requests exceeding threshold (default: 20ms)

# Codegen settings
codegen_watch_debounce_ms: 200 # Debounce file changes in watch mode (default: 200ms)
enable_schema_cache: true      # Enable two-tier schema cache (default: true)
```

### Configuration Notes

- Configuration is discovered by searching current directory and parent directories for `graphql.yaml` or `graphql.yml`
- All file paths in the config are resolved relative to the config file location
- Schema files can be specified as single strings or arrays for multi-file schemas
- Include/exclude patterns support standard glob syntax (`**/*.ts`, `src/**/*.{ts,tsx}`)
- Projects are matched in order; the first matching project is used for each file
- Public fragments are imported from the project that defined them
- Schema types are imported from the schema that defined them

---

## Fragment Directives

### @public - Shareable Fragments Across Projects

Use `@public` to make fragments available for import in other projects within your monorepo:

```graphql
# In package-a/fragments.graphql
fragment UserFields on User @public {
  id
  name
  email
}
```

```graphql
# In package-b/query.graphql
query GetUser($id: ID!) {
  user(id: $id) {
    ...UserFields  # Imports from package-a
  }
}
```

Generated TypeScript types will automatically import the fragment type:

```typescript
// package-b/query.codegen.ts
import type { UserFields } from "@workspace/package-a";

export interface GetUserQuery {
  user: ({ __typename: "User" } & UserFields) | null;
}
```

**Configuration requirement:** Set the `import` field in your project config to specify how other projects should import from it:

```yaml
projects:
  - schema: "schema.graphql"
    include: "packages/package-a/**/*.graphql"
    import: "@workspace/package-a"  # Other projects import from here

  - schema: "schema.graphql"
    include: "packages/package-b/**/*.graphql"
    import: "@workspace/package-b"
```

### @type_only - Type-Only Fragments

Use `@type_only` for fragments that are only used for TypeScript types and never used in actual GraphQL queries:

```graphql
# Define reusable type-only fragment
fragment UserBaseFields on User @type_only {
  id
  name
}

# Spread it in another fragment to compose types
fragment UserWithEmail on User {
  ...UserBaseFields
  email
}
```

Generated types will include the fragment but **no AST** will be generated:

```typescript
// Only type definition, no DocumentNode/AST
export interface UserBaseFields {
  __typename: "User";
  id: string;
  name: string;
}

// Full fragment with AST
export interface UserWithEmail {
  __typename: "User";
  id: string;
  name: string;
  email: string;
}

export const UserWithEmailFragmentDocument = { /* AST */ };
```

This prevents warnings about unused fragments for these as the tool is not following use of the typescript types.
The LSP will warn if you accidentally use a `@type_only` fragment in a query and provide a code action to remove it.

---

## Contributing

### Development Setup

This project is organized as a Rust workspace with specialized crates:
- **`graphql-core`**: Core models (`DocumentState`), schema loading, and validation engine.
- **`graphql-features`**: LSP features (Hover, Completion, etc.) implemented as extension traits.
- **`graphql-codegen`**: TypeScript type generation logic.
- **`graphql-lsp`**: Language Server implementation using `tower-lsp`.

1. **Clone and install dependencies**
   ```bash
   git clone https://github.com/soundtrack/graphql-rust.git
   cd graphql-rust
   ```

2. **Build the project**
   ```bash
   cargo build --workspace
   ```

3. **Run tests**
   ```bash
   cargo test --workspace
   ```

### Testing Your Changes

#### CLI Testing

**Option 1: Using the local binary directly**
```bash
cargo build
./target/debug/graphql-rust check
./target/debug/graphql-rust codegen
```

**Option 2: Using the npm package with local build**
```bash
# Build release binary
cargo build --release

# Set up npm package to use local build
export GRAPHQL_RUST_LOCAL_BUILD=$(pwd)/target/release/graphql-rust
cd npm/@soundtrack/graphql-rust-cli
pnpm install

# Now pnpm graphql-rust uses your local build
cd /path/to/test/project
pnpm graphql-rust check
pnpm graphql-rust codegen
```

**Quick setup script:**
```bash
./scripts/setup-npm-dev.sh
```

#### Editor Testing

**VSCode:**
1. Make Rust changes and rebuild: `cargo build --release`
2. Restart the extension: `Cmd+Shift+P` → "GraphQL: Restart Server"
3. The extension will pick up the new binary

**Neovim:**
```lua
-- Point to your local build
cmd = { '/path/to/graphql-rust/target/release/graphql-rust', 'lsp' }
```

**IntelliJ:**
1. In LSP4IJ settings, set Command to the full path of your local binary
2. Restart the LSP server after rebuilding

### Code Quality

```bash
# Lint
cargo clippy

# Format
cargo fmt

# Benchmarks
make benchmark

# Update test baselines
make update-baselines
```

### Creating a Release

This project uses automated release workflows to build and publish artifacts for multiple platforms.

**1. Bump the version:**

```bash
# For bug fixes (0.1.0 → 0.1.1)
make release-patch

# For new features (0.1.0 → 0.2.0)
make release-minor

# For breaking changes (0.1.0 → 1.0.0)
make release-major
```

The release script will:
- Update version in `Cargo.toml`, `plugins/swc/Cargo.toml`, `editors/vscode/package.json`, and `npm/graphql-rust-cli/package.json`
- Update `Cargo.lock`
- Create a commit with message: `chore: bump version to X.Y.Z`
- Create a git tag: `vX.Y.Z`
- Ask for confirmation before making changes

**2. Push the changes and tag:**

```bash
# Push commit and tag together
git push && git push origin vX.Y.Z

# Or push all tags at once
git push && git push --tags
```

**3. GitHub Actions automatically:**
- Builds binaries for Linux (x86_64, ARM64)
- Builds binaries for macOS (Intel, Apple Silicon)
- Builds binaries for Windows (x86_64, ARM64)
- Builds SWC plugin for all platforms
- Builds VSCode extension (.vsix)
- Publishes NPM package to GitHub Packages
- Creates a GitHub Release with all artifacts attached

The release will be available at: `https://github.com/soundtrack/graphql-rust/releases`

---

## Troubleshooting

### Binary Not Found

**Error:** `graphql-rust: command not found`

**Solutions:**
- Ensure `@soundtrack/graphql-rust-cli` is installed: `pnpm add @soundtrack/graphql-rust-cli`
- Check PATH includes node_modules/.bin
- Try using full path: `./node_modules/.bin/graphql-rust`

### LSP Not Connecting

**Error:** Editor shows "GraphQL Rust: Not Running"

**Solutions:**
1. Check the LSP output panel for errors
2. Verify configuration file exists: `graphql.yaml`
3. Ensure schema file path is correct in config
4. Try restarting the LSP server
5. Increase log level in editor settings

### Schema Not Loading

**Error:** "Schema file not found" or validation errors

**Solutions:**
1. Verify schema path in `graphql.yaml`
2. Check schema file syntax is valid GraphQL
3. Ensure schema file exists at specified path
4. Run `graphql-rust check` for detailed errors

### Codegen Issues

**Error:** Types not generated or outdated types

**Solutions:**
1. Run `graphql-rust codegen --clean` to clear cache
2. Check for syntax errors in GraphQL files
3. Verify all referenced fragments are defined
4. Check output directory permissions

### VSCode Extension Issues

**Solutions:**
1. Set `graphql-rust.serverPath` to full binary path in settings
2. Check Output panel → "GraphQL Rust Language Server"
3. Restart extension: `Cmd+Shift+P` → "GraphQL: Restart Server"
4. Reinstall extension if issues persist

### Performance Issues

**Solutions:**
1. Reduce `watch_all_files` scope in config
2. Increase `lsp_codegen_throttle_ms`
3. Enable schema cache: `enable_schema_cache: true`
4. Exclude large directories with `exclude` patterns

---

## Advanced Topics

### LSP Request Tracing

Enable tracing to debug slow LSP requests:

```yaml
tracing:
  enabled: true
  threshold_ms: 20  # Trace requests exceeding 20ms
```

Logs appear in the LSP output panel.

### Schema Caching

The LSP caches schemas in two tiers:
- **Memory cache (L1):** Process lifetime, invalidated by file mtime
- **Disk cache (L2):** Persistent across runs in OS cache directory

Disable if needed:
```yaml
enable_schema_cache: false
```

### Multi-Project Workspaces

Configure multiple projects in monorepos:

```yaml
projects:
  - schema: "packages/api/schema.graphql"
    include: "packages/api/src/**/*.{ts,tsx}"
    import: "@myorg/api"

  - schema: "packages/web/schema.graphql"
    include: "packages/web/src/**/*.{ts,tsx}"
    import: "@myorg/web"
```

---

## License

MIT

## Repository

https://github.com/soundtrack/graphql-rust
