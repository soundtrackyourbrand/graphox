# Plugin Development Guide

This guide covers how to develop, test, and contribute to the GraphQL Rust build tool plugins (Babel and SWC).

## Quick Links

- [Babel Plugin](./plugins/babel/README.md)
- [SWC Plugin](./plugins/swc/README.md)
- [Architecture Overview](./architecture.md)
- [Contributing](../CONTRIBUTING.md)

## Project Structure

```
graphql-rust/
├── plugins/
│   ├── babel/               # Babel transformation plugin
│   │   ├── index.js         # Main plugin code
│   │   ├── index.test.js    # Unit tests
│   │   ├── package.json     # npm configuration
│   │   └── README.md        # User documentation
│   │
│   └── swc/                 # SWC transformation plugin
│       ├── node/            # Node.js package (@soundtrack/graphql-rust-swc)
│       │   ├── package.json
│       │   ├── src/
│       │   ├── test/
│       │   ├── bin/
│       │   └── wasm/        # Bundled WASM
│       │
│       └── rust/            # Rust crate source
│           ├── Cargo.toml
│           ├── src/
│           └── lib.rs
│
├── tests/
│   ├── fixtures/
│   │   └── swc_cli/        # SWC integration test fixtures
│   │       ├── graphql.yaml
│   │       ├── schema.graphql
│   │       └── src/
│   │           └── app.ts
│   │
│   └── integration/
│       └── swc_cli.rs       # SWC integration test
│
└── .github/workflows/
    └── plugins.yml          # CI for plugins
```

## Babel Plugin Development

### Setup

```bash
cd plugins/babel
pnpm install
```

### Running Tests

```bash
# Run all tests
pnpm test

# Run with coverage
pnpm test --coverage

# Run in watch mode
pnpm test --watch
```

### Test Structure

```typescript
// index.test.js
import { describe, it, expect } from 'vitest';
import plugin from './index.js';

const babel = require('@babel/core');

describe('Babel Plugin', () => {
  it('transforms graphql template literals', () => {
    const input = `
      import { graphql } from './graphql';
      const query = graphql(\`query GetUser { user { id } }\`);
    `;

    const output = babel.transformSync(input, {
      plugins: [[plugin, {
        manifestPath: './__generated__/manifest.json',
        outputDir: './__generated__'
      }]]
    });

    expect(output.code).toContain('GetUserDocument');
    expect(output.code).not.toContain('graphql(`');
  });

  it('handles gql tag alias', () => {
    // Test with gql tag
  });

  it('preserves non-graphql imports', () => {
    // Test that other imports are not modified
  });
});
```

### Adding a New Test

1. Add test case to `index.test.js`
2. Run `pnpm test` to verify
3. Add to CI test matrix

### Publishing to GitHub Packages

```bash
# Update version
# Edit package.json version

# Build
pnpm build

# Publish
pnpm publish --registry https://npm.pkg.github.com
```

## SWC Plugin Development

The SWC plugin is structured as a WASM wrapper with a Rust implementation and Node.js bindings:

```
plugins/swc/
├── rust/              # Rust SWC plugin implementation
│   ├── src/lib.rs    # Main plugin code
│   └── Cargo.toml    # Rust configuration
│
└── node/              # Node.js WASM wrapper
    ├── src/          # TypeScript wrapper code
    ├── wasm/         # Compiled WASM (gitignored)
    └── package.json  # NPM configuration
```

### Building the Plugin

**Quick Build (Development):**
```bash
cd plugins/swc/node

# Install dependencies
pnpm install

# Build WASM from Rust source
pnpm run build:wasm

# Build TypeScript
pnpm run build

# Or build everything
pnpm run build:all
```

**Rust-Only Build:**
```bash
cd plugins/swc/rust

# Add WASM target if not already installed
rustup target add wasm32-unknown-unknown

# Build with wasm-pack
wasm-pack build --target nodejs --out-dir ../node/wasm
```

### Running Tests

```bash
cd plugins/swc/node

# Run TypeScript/Node.js tests
pnpm test

# Run Rust tests
cd ../rust
cargo test
```

### Running Tests

```bash
# Node.js package tests
cd plugins/swc/node
pnpm test

# Rust unit tests
cd plugins/swc/rust
cargo test

# Integration tests (requires wasm32-wasip1)
cargo test --include-ignored
```

### Publishing to GitHub Packages

The Node.js package is published to GitHub Packages:

```bash
cd plugins/swc/node
npm publish --registry https://npm.pkg.github.com
```

### Building

```bash
cd plugins/swc

# Debug build (faster)
cargo build -p graphql-rust-swc-plugin

# Release build (for production/WASM)
cargo build -p graphql-rust-swc-plugin --target wasm32-wasip1 --release

# Output location:
# target/wasm32-wasip1/debug/graphql_rust_swc_plugin.wasm
# target/wasm32-wasip1/release/graphql_rust_swc_plugin.wasm
```

### Running Tests

```bash
cd plugins/swc

# Run unit tests (fast)
cargo test

# Run integration tests (slow, requires wasm32-wasip1)
cargo test --include-ignored

# Run with output
cargo test -- --nocapture

# Run specific test
cargo test test_visitor_basic
```

### Test Structure

```rust
// src/lib.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name() {
        // Unit test
    }

    #[test]
    #[ignore] // Mark slow tests
    fn slow_test() {
        // Integration test
    }
}
```

### Integration Tests

The SWC plugin has integration tests in `tests/integration/swc_cli.rs`:

```bash
# Run the integration test
cargo test --test swc_cli --include-ignored

# This test:
# 1. Runs codegen on test fixtures
# 2. Builds the WASM plugin
# 3. Runs SWC with the plugin
# 4. Verifies the output
```

### Creating New Integration Tests

1. Create test fixtures in `tests/fixtures/swc_cli/`
2. Add test to `tests/integration/swc_cli.rs`
3. Run with `--include-ignored`
4. Verify CI passes

### Publishing

**Automatic Publishing via Release Workflow:**

When you push a version tag (e.g., `v0.2.0`), the `release.yml` workflow automatically:
1. Creates a GitHub release
2. Builds all artifacts
3. Publishes NPM packages to GitHub Packages

**Manual Publishing (Development/Testing):**

```bash
# Run from workspace root to bump versions
./scripts/release.sh bump 0.2.0

# Manual build and publish (not recommended for production)
cd plugins/swc/node
pnpm run build:all
npm publish --registry https://npm.pkg.github.com
```

**Distribution:**
- **NPM Package**: `@soundtrack/graphql-rust-swc` (published to GitHub Packages)
- **Release Asset**: Standalone WASM file (`graphql_rust_swc_plugin.wasm`)
- **Version**: Always synced with main project via `release.sh`

## Continuous Integration

### CI Workflows

Three workflows handle the project:

**1. Main CI (`ci.yml`)** - Runs on every PR:
- Rust formatting and clippy checks
- Test suite across platforms (Ubuntu, macOS, Windows)
- SWC plugin tests (build WASM + run Node.js tests)

**2. Plugins CI (`plugins.yml`)** - Plugin-specific tests only:
- Babel plugin tests
- SWC plugin build (WASM + TypeScript)
- **Note**: No release logic here - publishing happens in `release.yml`

**3. Release (`release.yml`)** - Everything release-related:
- Creates GitHub release
- Builds binaries for all platforms (6 targets)
- Builds SWC WASM plugin
- Publishes to GitHub Packages:
  - `@soundtrack/graphql-rust-cli`
  - `@soundtrack/graphql-rust-swc`
- Uploads assets to release:
  - Platform binaries
  - WASM file
  - VSCode extension

**Workflow Architecture:**
```
┌─────────────┐     ┌─────────────┐     ┌──────────────────────────┐
│   ci.yml    │     │ plugins.yml │     │      release.yml         │
│  (PR/Push)  │     │  (PR/Push)  │     │      (Tags Only)         │
└──────┬──────┘     └──────┬──────┘     └───────────┬──────────────┘
       │                   │                        │
       ▼                   ▼                        ▼
┌─────────────┐     ┌─────────────┐     ┌──────────────────────────┐
│ Rust Tests  │     │ Build WASM  │     │ 1. Create Release        │
│ Formatting  │     │ Node Tests  │     │ 2. Build Binaries (6x)   │
│ Clippy      │     │             │     │ 3. Build SWC WASM        │
└─────────────┘     └─────────────┘     │ 4. Build VSCode Extension│
                                        │ 5. Publish GitHub Packages│
                                        │ 6. Upload Assets         │
                                        └──────────────────────────┘
```

**Release Flow:**
1. Push tag `v*`
2. `release.yml` triggers automatically
3. All build jobs run in parallel where possible
4. Publishing jobs wait for their dependencies
5. Everything completes in one workflow run

## Common Tasks

### Adding a New Transformation

1. **Babel**: Modify `plugins/babel/index.js`
   - Add visitor pattern entry
   - Add test case
   - Update README

2. **SWC**: Modify `plugins/swc/src/lib.rs`
   - Implement `VisitMut` trait
   - Add unit test
   - Add integration test if needed
   - Update README

### Debugging Transformations

#### Babel

```javascript
// Add debug logging
console.log('Processing:', state.file.opts.filename);

// Or use Babel's built-in debugging
const debug = require('debug')('graphql-rust:babel');
debug('Transforming %s', state.file.opts.filename);
```

#### SWC

```rust
// Add tracing
println!("Processing: {:?}", current_file);

// Or use tracing subscriber
tracing::info!("Processing file: {:?}", current_file);
```

### Benchmarking

```bash
# Babel plugin benchmark
cd plugins/babel
node bench/benchmark.js

# SWC plugin benchmark
cd plugins/swc
cargo bench
```

## Style Guidelines

### JavaScript (Babel Plugin)

- Use ES modules
- Follow existing code style
- Add JSDoc comments

### Rust (SWC Plugin)

- Follow `cargo fmt` formatting
- Add doc comments for public APIs
- Use meaningful test names

## Submitting Changes

1. Fork the repository
2. Create a feature branch
3. Add tests for your changes
4. Ensure all tests pass
5. Update documentation
6. Submit PR

## See Also

- [Babel Plugin README](../plugins/babel/README.md)
- [SWC Plugin Node.js Package](../plugins/swc/node/README.md)
- [SWC Plugin Rust Crate](../plugins/swc/rust/README.md)
