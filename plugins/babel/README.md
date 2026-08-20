# @graphox/babel-plugin

## Overview

Ensures GraphQL AST files are properly codesplit, preventing them from all ending up in the initial chunk.

## Why Use This Plugin?

Codegen already generates AST files at compile time. The problem is bundler behavior:

**Without plugin:**
```
Initial chunk: ALL generated AST files (~50KB+)
Lazy chunks: empty or minimal
```

**With plugin:**
```
Initial chunk: ~1KB (just imports)
Lazy chunks: each operation in its chunk
```

## Installation

```bash
pnpm add --save-dev @graphox/babel-plugin
```

## Quick Start

### 1. Configure graphox

```yaml
# graphox.yaml
output_dir: "__generated__"
projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
```

### 2. Run Codegen

```bash
pnpm graphox codegen
```

### 3. Configure Babel

```javascript
// babel.config.js
const path = require('path');

module.exports = {
  presets: ['@babel/preset-typescript'],
  plugins: [
    ['@graphox/babel-plugin', {
      manifestPath: path.resolve(__dirname, '__generated__/manifest.json'),
      outputDir: path.resolve(__dirname, '__generated__'),
      graphqlImportPaths: ['@/graphql']
    }]
  ]
};
```

## Metro (React Native)

Metro uses Babel transformers under the hood. Configure in `metro.config.js`:

```javascript
// metro.config.js
module.exports = {
  transformer: {
    babelTransformerPath: require.resolve('@graphox/babel-plugin'),
  },
};
```

For full compatibility, also configure in `babel.config.js`.

## Codesplitting Impact

| Configuration | Initial Chunk | Per-Lazy-Chunk |
|--------------|--------------|----------------|
| Without plugin | ~50KB+ (all AST) | ~1KB |
| With plugin | ~1KB | ~1KB |

## Multiple Projects

Register every output directory a build resolves against in one plugin entry, so
imports that cross a project boundary can be rewritten:

```js
['@graphox/babel-plugin', {
  outputs: [
    { outputDir: path.resolve(__dirname, '__generated__') },
    { outputDir: path.resolve(repoRoot, 'packages/playback/base/graphql') },
  ],
}]
```

Within the package owning an output, documents are imported by relative path.
Crossing a package boundary, the import goes through the package's public
specifier instead, because a relative path would reach past its subpath exports:

```js
// before
import { PlaybackDisplayFragmentDoc } from "@soundtrack/playback/graphql";
// after
import { PlaybackDisplayFragmentDoc } from "@soundtrack/playback/graphql/base.codegen";
```

That needs the owning package to export the files inside the output directory:

```json
{
  "name": "@soundtrack/playback",
  "exports": {
    "./graphql": "./graphql/index.ts",
    "./graphql/*": "./graphql/*"
  }
}
```

The specifier is inferred from the package's `name` and `exports`, with a warning
naming the exact entry to add when the wildcard is missing. Inference only fires
when exactly one `exports` subpath resolves to the output directory, so set
`importAlias` explicitly for a package without a usable `exports` field.

Document names and sources may repeat across outputs — resolution is scoped to
the entrypoint an import came from. Output directories must be distinct and must
not nest.

## Configuration Options

| Option | Type | Required | Description |
|--------|------|----------|-------------|
| `outputs` | `object[]` | Yes* | One entry per project; see below |
| `emitExtensions` | `string` | No | File extension for generated imports: `"none"` (default), `"ts"`, `"js"`, `"dts"` |

Each entry in `outputs`:

| Option | Type | Required | Description |
|--------|------|----------|-------------|
| `outputDir` | `string` | Yes | Directory containing generated files |
| `manifestPath` | `string` | No | Path to `manifest.json`. Defaults to `<outputDir>/manifest.json` |
| `manifestData` | `object[]` | No | Inline manifest data, used instead of `manifestPath` |
| `graphqlImportPaths` | `string[]` | No | Extra import paths to treat as GraphQL entrypoints |
| `importAlias` | `string` | No | Public specifier for this output. Inferred from the package's `name` and `exports` |
| `packageRoot` | `string` | No | Root of the package owning this output. Inferred as the directory of the nearest `package.json` |

*The single-output form — those fields at the top level instead of inside
`outputs` — is still accepted and behaves as a one-element `outputs`.

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

## When to Use

| Build Tool | Recommended Plugin |
|------------|-------------------|
| React Native (Metro) | Babel |
| Webpack | Babel |
| Create React App | Babel |
| Storybook | Babel |
| rsbuild | Use SWC plugin |
| Turbopack/Next.js | Use SWC plugin |

## See Also

- [SWC Plugin](../swc/README.md)
- [graphox CLI](../../README.md)
- [Configuration Guide](../../docs/configurations.md)
