# @soundtrack/graphql-rust-cli

NPM package for installing the `graphql-rust` CLI - a high-performance GraphQL toolset for TypeScript monorepos.

## Installation

```bash
pnpm add @soundtrack/graphql-rust-cli
# or
npm install @soundtrack/graphql-rust-cli
# or
yarn add @soundtrack/graphql-rust-cli
```

## Usage

Once installed, you can use the `graphql-rust` command:

```bash
# Start the Language Server
pnpm graphql-rust lsp

# Validate GraphQL files
pnpm graphql-rust check

# Generate TypeScript types
pnpm graphql-rust codegen
pnpm graphql-rust codegen --clean
pnpm graphql-rust codegen --watch

# Run performance benchmarks
pnpm graphql-rust benchmark
```

### Global Installation

```bash
pnpm add -g @soundtrack/graphql-rust-cli

# Now you can use it directly
graphql-rust lsp
graphql-rust check
graphql-rust codegen
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

Create a `graphql.yaml` file in your project root. See the [main documentation](https://github.com/soundtrack/graphql-rust#configuration) for details.

## Local Development

If you're developing the CLI itself, you can use a local build instead of downloading from releases:

### Option 1: Using Environment Variable (Recommended)

```bash
# Build the CLI locally
cd /path/to/graphql-rust
cargo build --release

# Set environment variable to point to your local build
export GRAPHQL_RUST_LOCAL_BUILD=/path/to/graphql-rust/target/release/graphql-rust

# Now install the npm package - it will use your local build
cd /path/to/your/project
pnpm add /path/to/graphql-rust/npm/@soundtrack/graphql-rust-cli
```

The install script will copy your local binary instead of downloading from GitHub releases.

### Option 2: Using pnpm link

```bash
# In the graphql-rust repository, build the binary
cargo build --release

# Set up the local binary
export GRAPHQL_RUST_LOCAL_BUILD=$(pwd)/target/release/graphql-rust
cd npm/@soundtrack/graphql-rust-cli
pnpm install
```

## Manual Binary Download

If automatic installation fails, you can manually download binaries from the [releases page](https://github.com/soundtrack/graphql-rust/releases).

## Environment Variables

- `GRAPHQL_RUST_LOCAL_BUILD`: Path to a local binary to use instead of downloading (useful for development)
- `GRAPHQL_RUST_DOWNLOAD_URL`: Override the download URL for the binary (useful for mirrors or custom builds)

## License

MIT

## Repository

https://github.com/soundtrack/graphql-rust
