# graphox-swc-plugin (Rust Crate)

## Overview

This is the Rust source for the SWC plugin. It compiles to WASM for use with Node.js.

**For TypeScript/JavaScript users, use the Node.js package:**

```bash
pnpm add @graphox/swc-plugin
```

See [@graphox/swc-plugin](../node/README.md) for usage instructions.

## For Contributors

This crate compiles to a WASM module for the Node.js package.

### Building

```bash
# Add WASM target
rustup target add wasm32-wasip1

# Debug build
cargo build

# Release build (for npm package)
cargo build --target wasm32-wasip1 --release

# Output location:
# target/wasm32-wasip1/debug/graphox_swc_plugin.wasm
# target/wasm32-wasip1/release/graphox_swc_plugin.wasm
```

### Testing

```bash
# Unit tests
cargo test

# Integration tests (requires WASM)
cargo test --include-ignored
```

### Version

This crate version must match the Node.js package version (`@graphox/swc-plugin`).

## See Also

- [@graphox/swc-plugin (npm)](../node/README.md)
- [Babel Plugin](../../babel/README.md)
- [graphox CLI](../../../README.md)
