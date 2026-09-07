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

### swc_core and @swc/core

`swc_core` fixes the plugin ABI the `.wasm` is compiled against, and an SWC host
only loads a plugin whose ABI its own `@swc/core` understands. Bumping it is a
support decision, not a routine refresh, so Dependabot is told to leave it alone
in `.github/dependabot.yml`.

Since `@swc/core` 1.15.0 the AST crosses the boundary as CBOR and the AST enums
carry an `Unknown` variant, which is what makes a plugin survive a host it was
not built against. That floor is where the current plugin loads from, and it did
not move across the `swc_core` 56 to 77 bump. It is not guaranteed to hold: a
future bump can raise it, so re-check before shipping one, by running the plugin
under the oldest `@swc/core` we intend to keep working.

The `Unknown` variants only exist because `.cargo/config.toml` sets
`--cfg swc_ast_unknown` for the whole dependency graph; the `#[cfg(swc_ast_unknown)]`
match arms in `src/lib.rs` are the runtime behaviour for an AST node a newer host
sends that this build does not know. See the comment in that file for why the flag
cannot live in a build script.

## See Also

- [@graphox/swc-plugin (npm)](../node/README.md)
- [Babel Plugin](../../babel/README.md)
- [graphox CLI](../../../README.md)

## License

MIT — see [LICENSE](../../../LICENSE).
