# Common Configurations

This document provides ready-to-use configuration examples for common use cases, from basic setup to advanced configurations.

## Table of Contents

- [Full Configuration Reference](#full-configuration-reference)
- [Single Project](#single-project)
- [Monorepo with Multiple Projects](#monorepo-with-multiple-projects)
- [Shared Fragments Across Projects](#shared-fragments-across-projects)
- [Custom Scalars](#custom-scalars)
- [Schema Types Only](#schema-types-only)
- [Selective Codegen](#selective-codegen)
- [Ignoring Deprecations](#ignoring-deprecations)
- [Performance Tuning](#performance-tuning)
- [LSP Request Tracing](#lsp-request-tracing)
- [Validation Rules](#validation-rules)

---

## Full Configuration Reference

```yaml
# Global output directory for generated types
output_dir: "__generated__"

# Custom scalar type mappings
scalars:
  DateTime: "Date"
  JSON: "Record<string, any>"
  BigInt: "string"

# Ignore specific deprecation reasons in validation
ignore_deprecations:
  - "EXPERIMENTAL"
  - "INTERNAL"

# Generate and reuse AST nodes for fragments (for smaller bundle sizes)
generate_ast_for_fragments: false

# Project configurations (required)
projects:
  - schema: "schema.graphql"                    # Single schema file
    include: "src/client/**/*.{ts,tsx}"         # Glob pattern(s)
    exclude: "**/*.test.ts"                      # Optional exclusions
    output_dir: "src/client/__generated__"       # Override global output_dir
    import: "@workspace/project-1"                # How other projects import fragments
    generate_permissions: true                   # Generate permission metadata
    codegen: true                                # Enable codegen for this project
    document_suffix: "Document"                  # Suffix for Document constants
    variables_suffix: "Variables"                # Suffix for Variables interfaces
    fragment_suffix: ""                          # Suffix for Fragment interfaces

  - schema:                                      # Multiple schema files
      - "schema/base.graphql"
      - "schema/auth.graphql"
    include:                                     # Multiple include patterns
      - "src/server/**/*.ts"
      - "lib/**/*.graphql"
    codegen: false                               # Disable codegen for this project

# Generate standalone schema types (optional)
schema_types:
  - schema: "schema.graphql"
    output: "types/schema.ts"
    import: "@workspace/schema"                  # How generated files import this

# LSP settings
lsp_automatic_codegen: true                       # Auto-run codegen on file changes
lsp_codegen_throttle_ms: 300                      # Throttle automatic codegen
watch_all_files: true                            # Watch all workspace files
tracing:
  enabled: true                                   # Enable LSP request tracing
  threshold_ms: 20                                # Trace requests exceeding threshold

# Codegen settings
codegen_watch_debounce_ms: 200                    # Debounce file changes in watch mode
enable_schema_cache: true                         # Enable two-tier schema cache
document_suffix: "Document"                       # Global suffix for Document constants
variables_suffix: "Variables"                     # Global suffix for Variables interfaces
fragment_suffix: ""                               # Global suffix for Fragment interfaces
```

### Configuration Notes

- Configuration is discovered by searching current directory and parent directories for `graphql.yaml` or `graphql.yml`
- All file paths in the config are resolved relative to the config file location
- Schema files can be specified as single strings or arrays for multi-file schemas
- Include/exclude patterns support standard glob syntax (`**/*.ts`, `src/**/*.{ts,tsx}`)
- Projects are matched in order; the first matching project is used for each file

---

## Single Project

The simplest configuration for a single GraphQL schema and operations.

```yaml
# graphql.yaml
output_dir: "__generated__"

projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
```

---

## Monorepo with Multiple Projects

Configure multiple projects in a monorepo, each with its own schema and source patterns.

```yaml
# graphql.yaml (root of monorepo)
output_dir: "__generated__"

projects:
  # API project
  - schema: "packages/api/schema.graphql"
    include: "packages/api/src/**/*.{ts,tsx}"
    output_dir: "packages/api/src/__generated__"
    import: "@myorg/api"

  # Web project
  - schema: "packages/web/schema.graphql"
    include: "packages/web/src/**/*.{ts,tsx}"
    output_dir: "packages/web/src/__generated__"
    import: "@myorg/web"

  # Mobile project
  - schema: "packages/mobile/schema.graphql"
    include: "packages/mobile/**/*.{ts,tsx,js,jsx}"
    output_dir: "packages/mobile/src/__generated__"
    import: "@myorg/mobile"
```

**Key points:**
- Projects are matched in order; the first matching project is used for each file
- Each project can have its own `output_dir` override
- The `import` field specifies how other projects import generated types

---

## Shared Fragments Across Projects

Share GraphQL fragments between projects using the `@public` directive.

**File: packages/api/fragments.graphql**
```graphql
fragment UserFields on User @public {
  id
  name
  email
}

fragment PostFields on Post @public {
  id
  title
  body
}
```

**File: packages/web/src/queries.graphql**
```graphql
query GetUser($id: ID!) {
  user(id: $id) {
    ...UserFields
  }
}

query GetPosts {
  posts {
    ...PostFields
  }
}
```

**Configuration:**
```yaml
# graphql.yaml
projects:
  - schema: "packages/api/schema.graphql"
    include: "packages/api/**/*.graphql"
    import: "@myorg/api"

  - schema: "packages/api/schema.graphql"  # Reuse API schema
    include: "packages/web/src/**/*.{ts,tsx}"
```

**Generated TypeScript:**
```typescript
// packages/web/src/queries.codegen.ts
import type { UserFields, PostFields } from "@myorg/api";

export interface GetUserQuery {
  user: ({ __typename: "User" } & UserFields) | null;
}

export interface GetPostsQuery {
  posts: Array<{ __typename: "Post" } & PostFields>;
}
```

---

## Custom Scalars

Map GraphQL scalar types to TypeScript types.

```yaml
# graphql.yaml
output_dir: "__generated__"

scalars:
  DateTime: "Date"
  JSON: "Record<string, any>"
  BigInt: "string"
  UUID: "string"
  Upload: "File"

projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
```

**GraphQL Schema:**
```graphql
scalar DateTime
scalar JSON
scalar BigInt
scalar UUID
scalar Upload
```

**Generated Types:**
```typescript
export interface MyQuery {
  createdAt: Date;
  metadata: Record<string, any>;
  bigNumber: string;
  userId: string;
  file: File;
}
```

---

## Schema Types Only

Generate standalone TypeScript types from the schema without operations.

```yaml
# graphql.yaml
output_dir: "__generated__"

schema_types:
  - schema: "schema.graphql"
    output: "types/schema.ts"
    import: "@myorg/schema"

projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
    codegen: false  # Disable codegen, only generate schema types
```

**Generated: types/schema.ts**
```typescript
export interface User {
  __typename: "User";
  id: string;
  name: string;
  email: string;
}

export interface Query {
  user: User | null;
  users: Array<User>;
}
```

---

## Selective Codegen

Disable codegen for specific projects or enable it only where needed.

```yaml
# graphql.yaml
output_dir: "__generated__"

projects:
  # Shared fragments - only parse, don't generate types
  - schema: "packages/shared/schema.graphql"
    include: "packages/shared/fragments/**/*.graphql"
    codegen: false

  # API server - full codegen
  - schema: "packages/api/schema.graphql"
    include: "packages/api/src/**/*.{ts,tsx}"
    generate_permissions: true

  # Web client - full codegen
  - schema: "packages/api/schema.graphql"  # Reuse API schema
    include: "packages/web/src/**/*.{ts,tsx}"
```

---

## Ignoring Deprecations

Suppress warnings for deprecated fields or types.

```yaml
# graphql.yaml
output_dir: "__generated__"

ignore_deprecations:
  - "EXPERIMENTAL"
  - "INTERNAL"
  - "UseNewFieldInstead"

projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
```

---

## Performance Tuning

Configure performance-related settings for large workspaces.

```yaml
# graphql.yaml
output_dir: "__generated__"

# LSP settings
lsp_automatic_codegen: true
lsp_codegen_throttle_ms: 500  # Increase throttle for large workspaces
watch_all_files: false  # Disable watching all files, only watch GraphQL files

# Codegen settings
codegen_watch_debounce_ms: 300
enable_schema_cache: true  # Enable two-tier schema cache

projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
```

---

## LSP Request Tracing

Debug slow LSP requests by enabling tracing.

```yaml
# graphql.yaml
output_dir: "__generated__"

tracing:
  enabled: true
  threshold_ms: 50  # Only trace requests exceeding 50ms

projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
```

Logs appear in the LSP output panel of your editor.

---

## Validation Rules

Enable additional validation rules for stricter checks.

```yaml
# graphql.yaml
output_dir: "__generated__"

rules:
  unique_operation_name: true
  no_duplicate_fields: true
  no_unused_fragments: true
  required_fields:
    id: true
    permissions: ["mutation"]

projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
```

See [Validation Rules](./rules.md) for full documentation.
