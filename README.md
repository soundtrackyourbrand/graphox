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

## Commands

```bash
# Start the Language Server
graphql-rust lsp

# Validate GraphQL files
graphql-rust check

# Generate TypeScript types
graphql-rust codegen
graphql-rust codegen --clean # Remove generated files and caches
graphql-rust codegen --watch # Watches and runs codegen of file changes

# Run performance benchmarks
graphql-rust benchmark
```

## Configuration

Create a `graphql.yaml`  file in your project root:

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

# LSP settings
lsp_automatic_codegen: true  # Auto-run codegen on file changes (default: true)
watch_all_files: true        # Watch all workspace files (default: true)

# Performance tuning
enable_schema_cache: true    # Enable two-tier schema cache (default: true)

tracing:
  enabled: true              # Enable LSP request tracing
  threshold_ms: 20           # Only trace requests exceeding threshold (default: 20ms)

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
```

### Notes

- Configuration is discovered by searching current directory and parent directories for `graphql.yaml` or `graphql.yml`
- All file paths in the config are resolved relative to the config file location
- Schema files can be specified as single strings or arrays for multi-file schemas
- Include/exclude patterns support standard glob syntax (`**/*.ts`, `src/**/*.{ts,tsx}`)
- Projects are matched in order; the first matching project is used for each file
- Public fragments are imported from the project that defined them
- Schema types are imported from the schema that defined them

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

## Development

### Building and Testing

```bash
# Build the project
cargo build

# Run all tests
cargo test

# Run linting
cargo clippy

# Format code
cargo fmt

# Run benchmarks
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
- Update version in `Cargo.toml`, `plugins/swc/Cargo.toml`, and `editors/vscode/package.json`
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
- Creates a GitHub Release with all artifacts attached

The release will be available at: `https://github.com/YOUR_USERNAME/graphql-rust/releases`

### VSCode Extension Development

See [editors/vscode/README.md](editors/vscode/README.md) for detailed instructions on building and installing the VSCode extension locally.
