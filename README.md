# Graphox

A high-performance GraphQL toolset for TypeScript monorepos, providing LSP, type generation, and validation.

## Table of Contents

- [Features](#features)
- [Quick Start](#quick-start)
- [Installation](#installation)
- [Editor Setup](#editor-setup)
- [Commands](#commands)
- [Configuration](#configuration)
- [Fragment Directives](#fragment-directives)
- [Build Tool Plugins](./docs/plugins.md)
- [Validation Rules](./docs/rules.md)
- [Architecture](./docs/architecture.md)
- [Common Configurations](./docs/configurations.md)
- [Comparison with GraphQL Code Generator](./docs/comparison-graphql-codegen.md)
- [Contributing](./CONTRIBUTING.md)
- [Plugin Development](./docs/plugin-development.md)
- [License](#license)

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
   pnpm add @graphox/cli
   ```

2. **Create configuration**
   ```yaml
   # graphox.yaml
   projects:
     - schema: "schema.graphql"
       include: "src/**/*.{ts,tsx}"
       output_dir: "__generated__"
   ```

3. **Set up your editor** - See [Editor Setup](#editor-setup)

4. **Run commands**
   ```bash
   pnpm graphox check     # Validate GraphQL files
   pnpm graphox codegen   # Generate TypeScript types
   pnpm graphox lsp       # Start LSP (for editors)
   ```

---

## Installation

### NPM Package (Recommended)

Install via pnpm to automatically download the correct binary for your platform:

```bash
pnpm add @graphox/cli
npm install @graphox/cli
yarn add @graphox/cli
```

Then use with pnpm:

```bash
pnpm graphox lsp
pnpm graphox check
pnpm graphox codegen
```

Or install globally:

```bash
pnpm add -g @graphox/cli
graphox lsp
graphox check
graphox codegen
```

### Manual Binary Installation

Download pre-built binaries from the [releases page](https://github.com/soundtrackyourbrand/graphox/releases) for:
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

Set up `Graphox` as a language server in your editor:

| Editor | Setup Guide |
|--------|-------------|
| VSCode | [editors/vscode/README.md](editors/vscode/README.md) |
| Neovim | [editors/neovim.md](editors/neovim.md) |
| IntelliJ | [editors/intellij.md](editors/intellij.md) |

### Quick Editor Configuration

**VSCode:** Install the [Graphox extension](https://marketplace.visualstudio.com/items?itemName=soundtrack.graphox) or use the npm package.

**Neovim:** Configure LSP with `nvim-lspconfig`:

```lua
require('lspconfig').graphox.setup({
  cmd = { 'pnpm', 'exec', 'graphox', 'lsp' },
  filetypes = { 'graphql', 'typescript', 'typescriptreact' },
})
```

**IntelliJ/JetBrains:** Install LSP4IJ plugin and configure to run `pnpm exec graphox lsp`.

---

## Commands

```bash
# Start the Language Server
graphox lsp

# Validate GraphQL files
graphox check

# Generate TypeScript types
graphox codegen
graphox codegen --clean  # Remove generated files and caches
graphox codegen --watch   # Watches and runs codegen of file changes

# Run performance benchmarks
graphox benchmark
```

### Command Options

- `check` - Validates all GraphQL files against the schema
- `codegen` - Generates TypeScript types for operations
- `lsp` - Starts the Language Server Protocol server

---

## Configuration

Create a `graphox.yaml` file in your project root.

### Basic Example

```yaml
projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
    exclude: "**/*.test.ts"
    output_dir: "__generated__"
```

### Full Configuration

See [Common Configurations](docs/configurations.md) for detailed examples including:
- Single and multi-project setups
- Custom scalars and schema types
- Performance tuning and LSP tracing
- Monorepo patterns with shared fragments

### Configuration Notes

- Configuration is discovered by searching current directory and parent directories for `graphox.yaml` or `graphox.yml`
- All file paths in the config are resolved relative to the config file location
- Schema files can be specified as single strings or arrays for multi-file schemas
- Include/exclude patterns support standard glob syntax (`**/*.ts`, `src/**/*.{ts,tsx}`)
- Projects are matched in order; the first matching project is used for each file

See [Validation Rules](docs/rules.md) for configuring validation rules.

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

See [CONTRIBUTING.md](./CONTRIBUTING.md) for detailed information on:
- Development setup and prerequisites
- Testing your changes (CLI and editor)
- Code quality commands
- Release process

---

## Advanced Topics

See [docs/architecture.md](./docs/architecture.md) for in-depth documentation on:
- System architecture and components
- Data flow and processing pipeline
- Performance characteristics
- Caching strategy

See [docs/configurations.md](./docs/configurations.md) for:
- Multi-project workspace setup
- Custom scalar mappings
- Performance tuning
- LSP tracing configuration

---

## Repository

https://github.com/soundtrackyourbrand/graphox

## License

MIT — see [LICENSE](./LICENSE).
