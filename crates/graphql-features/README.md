# graphql-features

Implementation of GraphQL-specific intelligence and Language Server capabilities.

## Extension Trait Pattern

This crate uses extension traits to add LSP features to `DocumentState` (defined in `graphql-core`). This decoupling allows the core models to remain lean and makes it easier to test individual features.

### Example

To use hover support:

```rust
use graphql_features::hover::DocumentHover;

let hover_info = document.get_hover_info(params, schema, engine);
```

## Features

- **Autocomplete**: Context-aware suggestions for fields, arguments, types, and directives.
- **Diagnostics**: Real-time validation using both Tree-sitter and `apollo-compiler`.
- **Navigation**: Go-to-definition and Find References for fragments and types.
- **Hover**: Rich documentation and type information for GraphQL entities.
- **Semantic Tokens**: Enhanced syntax highlighting based on semantic analysis.

