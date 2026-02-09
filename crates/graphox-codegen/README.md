# graphox-codegen

Standalone crate for generating type-safe TypeScript code from GraphQL operations.

## Features

- **Type Generation**: Generates TypeScript interfaces for query, mutation, and subscription results.
- **Fragment Support**: Automatically handles fragment spreads and generates shared types for `@public` fragments.
- **Performance**: Uses `rayon` for parallel generation across large workspaces.
- **Validation**: Ensures operations are valid against the schema before generating code.

## CLI Usage

While this crate provides the core logic, it is usually invoked via the main `Graphox` CLI:

```bash
graphox codegen
```

