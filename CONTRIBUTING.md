# Contributing to Graphox

Thank you for your interest in contributing to Graphox! This guide covers development setup, testing, and release processes.

## Development Setup

This project is organized as a Rust workspace with specialized crates:
- **`graphox-core`**: Core models (`DocumentState`), schema loading, and validation engine.
- **`graphox-features`**: LSP features (Hover, Completion, etc.) implemented as extension traits.
- **`graphox-codegen`**: TypeScript type generation logic.
- **`graphox-lsp`**: Language Server implementation using `tower-lsp`.

### Prerequisites

- Rust 1.70+ with `cargo`
- Node.js 18+ and pnpm (for editor extensions and plugins)
- Git

### Getting Started

1. **Clone and install dependencies**
   ```bash
   git clone https://github.com/soundtrackyourbrand/graphox.git
   cd graphox
   ```

2. **Build the project**
   ```bash
   cargo build --workspace
   ```

3. **Run tests**
   ```bash
   cargo test --workspace
   ```

---

## Testing Your Changes

### CLI Testing

**Option 1: Using the local binary directly**
```bash
cargo build
./target/debug/graphox check
./target/debug/graphox codegen
```

**Option 2: Using the npm package with local build**
```bash
# Automated setup (Recommended)
./scripts/setup-npm-dev.sh

# Manual setup
cargo build --release
export GRAPHOX_LOCAL_BUILD=$(pwd)/target/release/graphox
cd npm/graphox-cli
node postinstall.js

# To test globally
pnpm link --global
```

Now you can link it in any test project:
```bash
cd /path/to/test/project
pnpm link --global @graphox/cli
graphox check
```

### Editor Testing

**VSCode:**
1. Make Rust changes and rebuild: `cargo build --release`
2. Restart the extension: `Cmd+Shift+P` → "GraphQL: Restart Server"
3. The extension will pick up the new binary

**Neovim:**
```lua
-- Point to your local build
cmd = { '/path/to/graphox/target/release/graphox', 'lsp' }
```

**IntelliJ:**
1. In LSP4IJ settings, set Command to the full path of your local binary
2. Restart the LSP server after rebuilding

---

## Code Quality

```bash
# Lint
cargo clippy

# Format
cargo fmt

# Benchmarks
make benchmark

# Update test baselines
make update-baselines

# Check everything at once
make check
```

---

## Creating a Release

This project uses automated release workflows to build and publish artifacts for multiple platforms.

### 1. Bump the version

```bash
# For bug fixes (0.1.0 → 0.1.1)
make release-patch

# For new features (0.1.0 → 0.2.0)
make release-minor

# For breaking changes (0.1.0 → 1.0.0)
make release-major
```

The release script will:
- Update version in `Cargo.toml`, `plugins/swc/Cargo.toml`, `editors/vscode/package.json`, and `npm/graphox-cli/package.json`
- Update `Cargo.lock`
- Create a commit with message: `chore: bump version to X.Y.Z`
- Create a git tag: `vX.Y.Z`
- Ask for confirmation before making changes

### 2. Push the changes and tag

```bash
# Push commit and tag together
git push && git push origin vX.Y.Z

# Or push all tags at once
git push && git push --tags
```

### 3. GitHub Actions automatically:
- Builds binaries for Linux (x86_64, ARM64)
- Builds binaries for macOS (Intel, Apple Silicon)
- Builds binaries for Windows (x86_64, ARM64)
- Builds SWC plugin for all platforms
- Builds VSCode extension (.vsix)
- Publishes NPM packages to the NPM registry
- Creates a GitHub Release with all artifacts attached

The release will be available at: `https://github.com/soundtrackyourbrand/graphox/releases`

---

## Project Structure

```
graphox/
├── crates/
│   ├── graphox-core/        # Core models and validation
│   ├── graphox-features/    # LSP features (extension traits)
│   ├── graphox-codegen/     # TypeScript type generation
│   ├── graphox-lsp/         # LSP server implementation
│   └── graphox-cli/         # CLI commands
├── editors/
│   ├── vscode/              # VSCode extension
│   ├── neovim.md            # Neovim configuration
│   └── intellij.md          # IntelliJ/LSP4IJ configuration
├── plugins/
│   ├── babel/               # Babel transformation plugin
│   └── swc/                 # SWC transformation plugin (Rust/WASM)
├── docs/                    # Documentation
│   ├── architecture.md      # Architecture overview
│   ├── configurations.md    # Common configurations
│   ├── plugins.md           # Build tool plugins
│   ├── rules.md             # Validation rules
│   └── plugin-development.md # Plugin development
└── scripts/                 # Build and release scripts
```

---

## License

By contributing to Graphox, you agree that your contributions will be licensed
under the [MIT License](./LICENSE) that covers the project.

## See Also

- [Architecture Documentation](./docs/architecture.md)
- [Plugin Development Guide](./docs/plugin-development.md)
- [API Documentation](https://docs.rs/graphox)
