# graphox-swc-plugin

## Overview

SWC plugin for Graphox codesplitting.

**For TypeScript/JavaScript users:**

```bash
pnpm add @graphox/swc-plugin
```

See [node/README.md](node/README.md) for usage instructions.

## Structure

```
swc/
├── node/          # Node.js package (@graphox/swc-plugin)
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

- [@graphox/swc-plugin (npm)](node/README.md)
- [Babel Plugin](../babel/README.md)
- [graphox CLI](../../README.md)
