# @graphox/cli

NPM package for installing the `Graphox` CLI - a high-performance GraphQL toolset for TypeScript monorepos.

## Installation

```bash
pnpm add @graphox/cli
# or
npm install @graphox/cli
# or
yarn add @graphox/cli
```

## Usage

Once installed, you can use the `Graphox` command:

```bash
# Start the Language Server
pnpm graphox lsp

# Validate GraphQL files
pnpm graphox check
pnpm graphox check apps/web          # Report only files under a directory

# Generate TypeScript types
pnpm graphox codegen
pnpm graphox codegen --clean
pnpm graphox codegen --watch
pnpm graphox codegen packages/schema  # Only the projects and schema_types under a directory

# Run performance benchmarks
pnpm graphox benchmark
```

### Global Installation

```bash
pnpm add -g @graphox/cli

# Now you can use it directly
graphox lsp
graphox check
graphox codegen
```

## Supported Platforms

This package automatically downloads the correct binary for your platform:

- **macOS**: x86_64 (Intel) and ARM64 (Apple Silicon)
- **Linux**: x86_64 and ARM64
- **Windows**: x86_64 and ARM64

## Features

- **Language Server (LSP)**: Real-time GraphQL validation, autocomplete, go-to-definition, hover docs, and more
- **Type Generation**: TypeScript type generation from GraphQL operations
- **Validation**: Granular diagnostics for GraphQL schemas and operations
- **Fragment Tracking**: Automatic fragment dependency resolution across packages

## Configuration

Create a `graphox.yaml` file in your project root. See the [main documentation](https://github.com/soundtrackyourbrand/graphox#configuration) for details.

### Editor Support

To get validation and autocomplete in your editor, add the following line at the top of your `graphox.yaml` file:

```yaml
# yaml-language-server: $schema=node_modules/@graphox/cli/graphox.schema.json
```

Or, if you use VS Code with the [YAML extension](https://marketplace.visualstudio.com/items?itemName=redhat.vscode-yaml), you can add the following to your workspace settings:

```json
{
  "yaml.schemas": {
    "node_modules/@graphox/cli/graphox.schema.json": "graphox.yaml"
  }
}
```

## Manual Binary Download

If automatic installation fails, you can manually download binaries from the [releases page](https://github.com/soundtrackyourbrand/graphox/releases).

## Repository

https://github.com/soundtrackyourbrand/graphox

## License

MIT — see [LICENSE](./LICENSE).
