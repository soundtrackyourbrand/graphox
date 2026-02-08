# graphql-rust-swc-plugin

## Overview

SWC plugin for GraphQL Rust codesplitting.

**For TypeScript/JavaScript users:**

```bash
pnpm add @soundtrack/graphql-rust-swc
```

See [node/README.md](node/README.md) for usage instructions.

## Structure

```
swc/
├── node/          # Node.js package (@soundtrack/graphql-rust-swc)
│   ├── package.json
│   ├── src/
│   └── wasm/      # Bundled WASM
│
└── rust/          # Rust crate source
    ├── Cargo.toml
    ├── src/
    └── README.md
```

## For Contributors

See [rust/README.md](rust/README.md) for Rust development instructions.

## See Also

- [@soundtrack/graphql-rust-swc (npm)](node/README.md)
- [Babel Plugin](../babel/README.md)
- [graphql-rust CLI](../../README.md)
