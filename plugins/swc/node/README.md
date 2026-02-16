# @soundtrack/graphox-swc

## Overview

Pre-built SWC plugin for Graphox codesplitting. This package bundles the WASM binary for easy use with rsbuild, Turbopack, or native SWC.

## Prerequisites

Building the WASM plugin from source requires:

- **Rust toolchain** (1.70+): `rustup install stable`
- **WASM target**: `rustup target add wasm32-wasip1`
- **wasm-pack**: `cargo install wasm-pack`
- **Node.js** 18+
- **pnpm**: `corepack enable && corepack install -g pnpm@latest`

For users of the pre-built package, only Node.js 18+ is required.

## Installation

```bash
pnpm add @soundtrack/graphox-swc
```

## Requirements

- Node.js 18+
- rsbuild, Turbopack, or native SWC

## Usage

### rsbuild Configuration

```typescript
// rsbuild.config.ts
import { defineConfig } from '@rsbuild/core';
import { createSWCPlugin } from '@soundtrack/graphox-swc';
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
import { createSWCPlugin } from '@soundtrack/graphox-swc';

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

- [Babel Plugin](../babel/README.md)
- [graphox CLI](../../README.md)
- [Plugin Development Guide](../../../docs/plugin-development.md)
