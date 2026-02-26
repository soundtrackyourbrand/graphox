# Comparison with GraphQL Code Generator

While `graphql-codegen` is the industry standard for GraphQL type generation, `graphox` was built from the ground up in Rust to address specific pain points in large-scale TypeScript environments, particularly regarding performance, monorepo management, and IDE integration.

## At a Glance

| Feature | GraphQL Code Generator | Graphox |
| :--- | :--- | :--- |
| **Language** | JavaScript/TypeScript (Node.js) | Rust |
| **Performance** | Good (can be slow in large projects) | Ultra-fast (multi-threaded, Rust) |
| **LSP Support** | Available via separate plugins | Built-in, native LSP first |
| **Monorepo** | One config per package (standard) | Native multi-project single config |
| **Fragment Masking** | Via `client-preset` | Built-in, first-class support |
| **Watch Mode** | Polls or uses `chokidar` | OS-native events (`notify` crate) |
| **Ecosystem** | Massive (Java, C#, Flutter, etc.) | TypeScript focused |

---

## What Graphox Improves

### 1. Monorepo Experience
In a monorepo, `graphql-codegen` often requires a `codegen.yml` in every package or a complex root configuration that spawns multiple processes.

**Graphox** uses a single `graphox.yaml` at the root. It scans the entire workspace once, builds a global fragment map, and resolves dependencies across projects natively. 
- **Single Process:** One process handles all projects in the monorepo.
- **Cross-Project Fragments:** Fragments defined in one package and marked `@public` are automatically available and correctly imported in other packages.
- **Unified Configuration:** Manage all project schemas and output paths in one place.

### 2. Performance
`graphox` is significantly faster than `graphql-codegen`, especially as the number of GraphQL documents grows.
- **Parallelism:** Uses Rust's `rayon` for parallel parsing and generation.
- **Incremental Codegen:** The built-in LSP performs incremental updates, only re-generating types for affected files.
- **Two-Tier Caching:** Implements advanced caching for schema analysis and type conversion to avoid redundant work.

### 3. Native Cross-Project Fragment Sharing
In a monorepo, sharing fragments between packages usually requires complex build steps or duplicated code. `graphox` introduces the `@public` directive:
- **@public Fragments:** Mark a fragment as public in one project, and it becomes available in any other project in the workspace.
- **Automatic Imports:** `graphox` automatically generates the correct TypeScript imports between your packages, maintaining a single source of truth for your data requirements across the entire monorepo.

### 4. Integrated LSP
`graphox` isn't just a CLI tool; it's a Language Server. This means your IDE validation and your codegen are powered by the same engine.
- **Instant Feedback:** Errors appear in your editor as you type.
- **Automatic Codegen:** Codegen can run automatically in the background as you edit files, with sophisticated debouncing to keep your machine quiet.

---

## Shared Features

### Fragment Masking
Both `graphql-codegen` (via `client-preset`) and `graphox` provide excellent support for Fragment Masking. This pattern enforces explicit data dependencies and prevents "prop drilling" of large objects. In both tools, this is a first-class citizen designed to improve the maintainability of large React/TypeScript applications.

---

## What Graphox is Missing

Despite its advantages, `graphox` is a more specialized tool and lacks some of the breadth of `graphql-codegen`:

### 1. Ecosystem & Frameworks
`graphql-codegen` has a massive plugin ecosystem for almost everything:
- **Frameworks:** Vue, Svelte, Elm, etc.
- **Target Languages:** Java, Kotlin, C#, Python, Flutter/Dart.
- **Libraries:** Specific wrappers for React Query, SWR, URQL.

`graphox` is strictly focused on **TypeScript/JavaScript** and **Apollo Client** style workflows.

### 2. No Plugin API
`graphql-codegen` allows users to write custom plugins in JavaScript to transform output. **`graphox` does not have a plugin API**, and plugin support is not currently supported. The core is written in Rust for maximum performance, and all generation logic is built-in.

### 3. Limited Legacy Support
`graphql-codegen` supports a wide variety of legacy GraphQL patterns and configuration styles. `graphox` intentionally does not support:
- **String-based Types:** `graphox` only generates modern TypeScript interfaces and `TypedDocumentNode` ASTs.
- **Global Types:** It does not support generating a single massive `global.d.ts`.
- **Legacy Decorators:** There is no support for older `graphql-tag/loader` or non-standard decorators.
- **Custom Templates:** Since there is no plugin API, you cannot customize the generated code structure beyond the provided configuration options.

---

## Which one should I choose?

**Choose `graphox` if:**
- You are working in a large TypeScript monorepo.
- You want the fastest possible developer experience and IDE feedback.
- You are frustrated with the setup complexity of codegen in multi-project environments.
- You use modern patterns like Fragment Masking.

**Choose `graphql-codegen` if:**
- You need to generate code for languages other than TypeScript/JavaScript.
- You rely on a specific plugin that doesn't have an equivalent in `graphox`.
- You need deep customization of the output through custom JavaScript plugins.
