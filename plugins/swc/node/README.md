# @graphox/swc-plugin

## Overview

Pre-built SWC plugin for Graphox codesplitting. This package bundles the WASM binary for easy use with rsbuild, Turbopack, or native SWC.

## Prerequisites

Building the WASM plugin from source requires:

- **Rust toolchain**: pinned in `rust-toolchain.toml` — `rustup` installs it,
  and the `wasm32-wasip1` target, automatically
- **Node.js** 22.13+ (required by pnpm 11)
- **pnpm** 11: `corepack enable && corepack install -g pnpm@latest`

For users of the pre-built package, only Node.js 18+ is required.

## Installation

```bash
pnpm add @graphox/swc-plugin
```

## Requirements

- Node.js 18+
- rsbuild, Turbopack, or native SWC

## Usage

### rsbuild Configuration

```typescript
// rsbuild.config.ts
import { defineConfig } from '@rsbuild/core';
import { createSWCPlugin } from '@graphox/swc-plugin';
import path from 'path';

export default defineConfig({
  source: {
    alias: {
      '__generated__': path.resolve(__dirname, './__generated__'),
    },
  },
  tools: {
    swc: {
      jsc: {
        parser: {
          syntax: 'typescript',
          tsx: true,
        },
        experimental: {
          plugins: [
            createSWCPlugin({
              manifestPath: './__generated__/manifest.json',
              outputDir: './__generated__'
            })
          ],
        },
      },
    },
  },
});
```

### Turbopack/Next.js Configuration

```javascript
// next.config.js
import { createSWCPlugin } from '@graphox/swc-plugin';

/** @type {import('next').NextConfig} */
const nextConfig = {
  experimental: {
    turbo: {
      rules: {
        '*.{ts,tsx}': [
          {
            loader: 'next-swc-loader',
            options: {
              jsc: {
                parser: {
                  syntax: 'typescript',
                  tsx: true,
                },
                experimental: {
                  plugins: [
                    createSWCPlugin({
                      manifestPath: './__generated__/manifest.json',
                      outputDir: './__generated__'
                    })
                  ],
                },
              },
            },
          },
        ],
      },
    },
  },
};

module.exports = nextConfig;
```

## Multiple Projects

A workspace whose modules resolve GraphQL against several output directories
should register them all in **one** plugin instance:

```ts
graphox.createSWCPlugin({
  outputs: [
    { outputDir: path.resolve(__dirname, 'app/graphql') },
    { outputDir: path.resolve(repoRoot, 'packages/auth/graphql') },
    { outputDir: path.resolve(repoRoot, 'packages/catalog/graphql') },
  ],
})
```

One instance per output is expensive and incomplete:

- **Cost.** SWC serializes the whole AST into the plugin and back for every
  plugin on every module, whether or not the plugin changes anything. A module
  belongs to exactly one output, so with N instances, N−1 of those round-trips
  per module are guaranteed no-ops.
- **Correctness.** An instance only knows its own project, so it cannot rewrite
  an import that crosses a project boundary. With every output registered
  together, a document imported from another project's entrypoint is redirected
  straight at that project's generated file.

### Cross-project imports

Within the package that owns an output, documents are imported by relative path.
Crossing a package boundary, a relative path would reach past the package's
subpath exports, so the import goes through the package's public specifier
instead:

```ts
// packages/storefront/graphql/storefront.codegen.ts, before
import { ProductCardFragmentDoc } from "@example/catalog/graphql";

// after
import { ProductCardFragmentDoc } from "@example/catalog/graphql/catalog.codegen";
```

That requires the owning package to export the files inside its output
directory, not just the entrypoint:

```json
{
  "name": "@example/catalog",
  "exports": {
    "./graphql": "./graphql/index.ts",
    "./graphql/*": "./graphql/*"
  }
}
```

The plugin infers the specifier from the package's `name` and `exports`, and
warns naming the exact entry to add when the wildcard is missing. Inference is
deliberately conservative — it only fires when exactly one `exports` subpath
resolves to the output directory — so set `importAlias` explicitly for a package
with no `exports` field, or one resolved only through bundler aliases.

Document names and sources may repeat across outputs. Two projects defining
`SetPriceMutationDocument`, even with identical source text, is fine:
resolution is scoped to the entrypoint an import came from rather than searched
across a merged map.

Output directories must be distinct, and must not nest.

## Configuration Options

| Option | Type | Required | Description |
|--------|------|----------|-------------|
| `outputs` | `object[]` | Yes* | One entry per project; see below. Preferred over the single-output fields |
| `emitExtensions` | `string` | No | File extension for generated imports: `"none"` (default), `"ts"`, `"js"`, `"dts"` |

Each entry in `outputs`:

| Option | Type | Required | Description |
|--------|------|----------|-------------|
| `outputDir` | `string` | Yes | Directory containing generated files |
| `manifestPath` | `string` | No | Path to `manifest.json`. Defaults to `<outputDir>/manifest.json` |
| `manifestData` | `object[]` | No | Inline manifest data, used instead of `manifestPath` |
| `graphqlImportPaths` | `string[]` | No | Extra import paths to treat as GraphQL entrypoints |
| `importAlias` | `string` | No | Public specifier for this output, e.g. `@example/catalog/graphql`. Inferred from the package's `name` and `exports` |
| `packageRoot` | `string` | No | Root of the package owning this output. Inferred as the directory of the nearest `package.json` |

*The single-output form — `outputDir`, `manifestPath`, `manifestData`,
`graphqlImportPaths`, `importAlias` and `packageRoot` at the top level — is still
accepted and behaves as a one-element `outputs`.

### Local Development

You can override the WASM plugin path by setting the `GRAPHOX_SWC_PLUGIN_PATH` environment variable. This is useful for testing local builds of the Rust plugin without copying files.

```bash
export GRAPHOX_SWC_PLUGIN_PATH=$(pwd)/target/wasm32-wasip1/release/graphox_swc_plugin.wasm
```

### emitExtensions

Controls the file extension appended to generated import paths. Should match the `emit_extensions` setting in your `graphox.yaml`:

| Value | Result |
|-------|--------|
| `"none"` (default) | `import { X } from "./file.codegen"` |
| `"ts"` | `import { X } from "./file.codegen.ts"` |
| `"js"` | `import { X } from "./file.codegen.js"` |
| `"dts"` | `import { X } from "./file.codegen.d.ts"` |

## Fragment Documents

When `generate_ast_for_fragments: true` is enabled in your config, fragment documents are also included in the manifest and will be properly rewritten by the plugin.

## Building from Source

If you need to rebuild the WASM plugin:

```bash
# Install dependencies
pnpm install

# Build TypeScript only (requires pre-built WASM)
pnpm run build

# Build WASM only (requires Rust toolchain)
pnpm run build:wasm

# Build everything (TypeScript + WASM)
pnpm run build:all

# Run tests
pnpm test
```

## See Also

- [Babel Plugin](../../babel/README.md)
- [graphox CLI](../../../README.md)
- [Configuration Guide](../../../docs/configurations.md)
- [Plugin Development Guide](../../../docs/plugin-development.md)

## License

MIT — see [LICENSE](./LICENSE).
