# Build Tool Plugins

Graphox provides plugins for **Babel** and **SWC** to ensure GraphQL AST files are properly codesplit, preventing them from all ending up in the initial chunk.

## Quick Decision Guide

Codegen already generates AST files at compile time. These plugins ensure proper codesplitting.

| Your Build Tool | Recommended Plugin |
|-----------------|-------------------|
| rsbuild | SWC Plugin |
| Next.js (Turbopack) | SWC Plugin |
| Native SWC | SWC Plugin |
| React Native (Metro) | Babel Plugin |
| Next.js (Webpack) | Babel Plugin |
| Create React App | Babel Plugin |
| Storybook | Babel Plugin |
| Custom Webpack | Babel Plugin |

## The Problem

**Without plugin:** All generated AST files get bundled into the initial chunk.

```
Initial chunk: ~50KB+ (all AST files)
Lazy chunks: ~1KB each
```

**With plugin:** Each operation stays in its own chunk.

```
Initial chunk: ~1KB (just imports)
Lazy chunks: ~1KB each (operations in their chunks)
```

## Plugin Comparison

| Feature | Babel Plugin | SWC Plugin |
|---------|--------------|------------|
| **Build Speed** | Slower | Faster |
| **Output** | Properly codesplit | Properly codesplit |
| **Rust Required** | No | Yes (for building WASM) |
| **WASM Available** | N/A | Yes (pre-built) |
| **Configuration** | JSON/JavaScript | JSON |

## Installation & Setup

Choose your build tool:

### [@graphox/babel-plugin →](../plugins/babel/README.md)
For Webpack-based projects and React Native (Metro)

### [@graphox/swc-plugin →](../plugins/swc/node/README.md)
For rsbuild, Turbopack, or native SWC

## See Also

- [graphox CLI Documentation](../README.md)
- [Editor Setup](../editors/README.md)
