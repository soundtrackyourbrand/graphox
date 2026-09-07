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

### swc_core and the host

`swc_core` fixes the plugin ABI the `.wasm` carries, and a host only runs a
plugin whose ABI its own `swc_core` understands. Bumping it is a support
decision, not a routine refresh, so Dependabot is told to leave it alone in
`.github/dependabot.yml`.

Since `@swc/core` 1.15.0 the AST crosses the boundary as CBOR and the AST enums
carry an `Unknown` variant. That buys compatibility in **one direction only**: a
plugin runs on a host newer than itself, because a node it does not recognise
arrives as `Unknown`. It does not run on an older host — a host that predates a
change to an existing node, such as `FunctionBody`, hands over a shape the
plugin cannot decode, and CBOR does not bridge that. So the rule is:

> every host must be on a `swc_core` at least as new as this crate's.

Raising `swc_core` therefore raises the floor under every host, and each has to
be checked separately, because they embed `swc_core` on their own schedules:

| host | floor for `swc_core` 77 |
| --- | --- |
| `@swc/core` | 1.15.0 |
| `@rspack/core` (`builtin:swc-loader`, and `@rsbuild/core` through it) | 2.2.0 |

rspack is the binding constraint: it trails `@swc/core` by several `swc_core`
majors, so it is the one to check first. When it is too old the build fails with
`failed to invoke plugin` and a hint naming rspack's own `swc_core`. The hint is
worth trusting; the failure is not otherwise obvious, and it only shows up on a
file whose AST actually contains a changed node — a source file with no function
declaration can transform fine against an incompatible host.

Both floors are covered by tests: `test_swc_cli_integration` pins `@swc/core` to
the floor, and the monorepo e2e fixture pins rspack through
`tests/fixtures/monorepo_e2e`. Re-measure both before shipping the next bump —
build the wasm and run it under the oldest host we intend to keep working,
against a file containing a function declaration — and move the pins and this
table together.

The `Unknown` variants only exist because `.cargo/config.toml` sets
`--cfg swc_ast_unknown` for the whole dependency graph; the `#[cfg(swc_ast_unknown)]`
match arms in `src/lib.rs` are the runtime behaviour for a node a newer host
sends that this build does not know. See the comment in that file for why the
flag cannot live in a build script.

## See Also

- [@graphox/swc-plugin (npm)](../node/README.md)
- [Babel Plugin](../../babel/README.md)
- [graphox CLI](../../../README.md)

## License

MIT — see [LICENSE](../../../LICENSE).
