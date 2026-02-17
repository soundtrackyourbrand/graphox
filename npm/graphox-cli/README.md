# @soundtrackyourbrand/graphox-cli

NPM package for installing the `Graphox` CLI - a high-performance GraphQL toolset for TypeScript monorepos.

## Installation

```bash
pnpm add @soundtrackyourbrand/graphox-cli
# or
npm install @soundtrackyourbrand/graphox-cli
# or
yarn add @soundtrackyourbrand/graphox-cli
```

## Usage

Once installed, you can use the `Graphox` command:

```bash
# Start the Language Server
pnpm graphox lsp

# Validate GraphQL files
pnpm graphox check

# Generate TypeScript types
pnpm graphox codegen
pnpm graphox codegen --clean
pnpm graphox codegen --watch

# Run performance benchmarks
pnpm graphox benchmark
```

### Global Installation

```bash
pnpm add -g @soundtrackyourbrand/graphox-cli

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

## Local Development

If you're developing the CLI itself, you can use a local build instead of downloading from releases:

### Option 1: Using Environment Variable (Recommended)

```bash
# Build the CLI locally
cd /path/to/graphox
cargo build --release

# Set environment variable to point to your local build
export GRAPHOX_LOCAL_BUILD=/path/to/graphox/target/release/graphox

# Now install the npm package - it will use your local build
cd /path/to/your/project
pnpm add /path/to/graphox/npm/graphox-cli
```

The install script will copy your local binary instead of downloading from GitHub releases.

### Option 2: Using pnpm link

```bash
# In the graphox repository, build the binary
cargo build --release

# Set up the local binary
./scripts/setup-npm-dev.sh

# Link globally
cd npm/graphox-cli
pnpm link --global
```

## Manual Binary Download

If automatic installation fails, you can manually download binaries from the [releases page](https://github.com/soundtrackyourbrand/graphox/releases).

## Environment Variables

- `GRAPHOX_LOCAL_BUILD`: Path to a local binary to use instead of downloading (useful for development)
- `GRAPHOX_DOWNLOAD_URL`: Override the download URL for the binary (useful for mirrors or custom builds)

## License

MIT

## Repository

https://github.com/soundtrackyourbrand/graphox
