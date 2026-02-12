# Common Configurations

This document provides ready-to-use configuration examples for common use cases, from basic setup to advanced configurations.

## Table of Contents

- [Full Configuration Reference](#full-configuration-reference)
- [Single Project](#single-project)
- [Monorepo with Multiple Projects](#monorepo-with-multiple-projects)
- [Shared Fragments Across Projects](#shared-fragments-across-projects)
- [Fragment Masking](#fragment-masking)
- [Custom Scalars](#custom-scalars)
- [Schema Types Only](#schema-types-only)
- [Apollo Client possibleTypes](#apollo-client-possibletypes)
- [Selective Codegen](#selective-codegen)
- [Ignoring Deprecations](#ignoring-deprecations)
- [Performance Tuning](#performance-tuning)
- [LSP Request Tracing](#lsp-request-tracing)
- [Validation Rules](#validation-rules)

---

## Full Configuration Reference

```yaml
# Fragment masking (similar to graphql-codegen client-preset)
# Disabled by default for backwards compatibility
fragment_masking: enabled  # or: disabled
# fragment_masking:
#   unmask_function_name: "getFragmentData"  # Custom function name

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
    emit_permission_data: true                   # Generate permission metadata
    codegen: true                                # Enable codegen for this project
    document_suffix: "Document"                  # Suffix for Document constants
    variables_suffix: "Variables"                # Suffix for Variables interfaces
    fragment_suffix: ""                          # Suffix for Fragment interfaces
    fragment_document_suffix: ""                 # Suffix for Fragment document constants (masking only)
    fragment_masking: disabled                   # Enable/disable fragment masking (default: disabled)
    possible_types: "graphql-introspection.ts"  # Generate possibleTypes for Apollo Client
    type_policies: "type-policies.ts"            # Generate TypePolicies for Apollo Client

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
    possible_types: "types/possible-types.ts"   # Generate possibleTypes for Apollo Client
    type_policies: "types/type-policies.ts"      # Generate TypePolicies for Apollo Client

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
fragment_document_suffix: ""                      # Global suffix for Fragment document constants (masking only)
```

### Configuration Notes

- Configuration is discovered by searching current directory and parent directories for `graphox.yaml` or `graphox.yml`
- All file paths in the config are resolved relative to the config file location
- Schema files can be specified as single strings or arrays for multi-file schemas
- Include/exclude patterns support standard glob syntax (`**/*.ts`, `src/**/*.{ts,tsx}`)
- Projects are matched in order; the first matching project is used for each file

---

## Single Project

The simplest configuration for a single GraphQL schema and operations.

```yaml
# graphox.yaml
projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
    output_dir: "__generated__"
```

---

## Monorepo with Multiple Projects

Configure multiple projects in a monorepo, each with its own schema and source patterns.

```yaml
# graphox.yaml (root of monorepo)
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
# graphox.yaml
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

## Fragment Masking

Enable fragment masking to prevent fragment fields from "leaking" into parent operation types. This pattern, similar to graphql-codegen's client-preset, enforces explicit data dependencies and improves type safety.

### Configuration

Fragment masking is **disabled by default** for backwards compatibility.

```yaml
# graphox.yaml
# Global setting (disabled by default)
fragment_masking: enabled

# Or with custom function name
fragment_masking:
  unmask_function_name: "getFragmentData"

projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
    output_dir: "__generated__"
```

### Per-Project Override

Override fragment masking per project:

```yaml
# graphox.yaml
fragment_masking: enabled  # Global default

projects:
  # Uses global (enabled)
  - schema: "schema.graphql"
    include: "src/app/**/*"

  # Overrides to disabled
  - schema: "schema.graphql"
    include: "src/admin/**/*"
    fragment_masking: disabled
```

### Generated Output

**Without Fragment Masking (default):**
```typescript
// Fragment
export interface UserFragment {
  __typename: "User";
  id: string;
  name: string;
}

// Query - fields "leak" into parent type
interface GetUserQuery {
  user: ({ __typename: "User" } & UserFragment) | null;
}

// Usage - direct access
const user: GetUserQuery["user"] = data.user;
console.log(user.name);  // Direct field access
```

**With Fragment Masking (enabled):**
```typescript
// Fragment - adds __fragment property
export interface UserFragment {
  __typename: "User";
  id: string;
  name: string;
}

export declare const UserFragment: {
  __fragment: UserFragment;
};

// fragment-masking.ts (generated)
export type FragmentType<TFragment> = TFragment extends { ' $fragmentRefs'?: { [key: string]: any } }
  ? TFragment
  : TFragment extends { ' $fragmentName'?: string }
  ? TFragment
  : TFragment extends { __fragment: infer T }
  ? T
  : never;

export function getFragmentData<TFragment>(
  _fragment: TFragment,
  data: FragmentType<TFragment>
): FragmentType<TFragment> {
  return data as any;
}

// Query - fragment spread becomes FragmentType
interface GetUserQuery {
  user: ({ __typename: "User" } & { ' $fragmentRefs'?: { 'UserFragment': UserFragment } }) | null;
}

// Usage - requires unmask function
import { getFragmentData } from "./fragment-masking";
import { UserFragment } from "./UserFragment.codegen";

const user = getFragmentData(UserFragment, data.user);
```

### Options

- `fragment_suffix`: Suffix for the fragment type (default: `""`).
- `fragment_document_suffix`: Suffix for the fragment document constant (default: same as `fragment_suffix`). This allows you to name your fragment type `UserFieldsFragment` and your document `UserFieldsDocument` for better clarity.

### Benefits

- **Type Safety**: Components can only access fields they explicitly request
- **Explicit Dependencies**: Component data requirements are colocated with the component
- **No Field Leakage**: Parent queries can't accidentally access fragment fields

### Migration from Disabled to Enabled

1. Enable fragment masking in config: `fragment_masking: enabled`
2. Update components to use `FragmentType<>` props
3. Replace direct field access with `getFragmentData()` calls

---

## Custom Scalars

Map GraphQL scalar types to TypeScript types.

```yaml
# graphox.yaml
scalars:
  DateTime: "Date"
  JSON: "Record<string, any>"
  BigInt: "string"
  UUID: "string"
  Upload: "File"

projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
    output_dir: "__generated__"
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
# graphox.yaml
schema_types:
  - schema: "schema.graphql"
    output: "types/schema.ts"
    import: "@myorg/schema"

projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
    codegen: false  # Disable codegen, only generate schema types
    output_dir: "__generated__"
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

## Apollo Client possibleTypes

Generate `possibleTypes` introspection data for Apollo Client's `InMemoryCache`. Apollo Client requires knowledge of interface and union type hierarchies to properly cache and merge results.

### Overview

When your GraphQL schema uses interfaces or unions, Apollo Client needs to know which concrete types implement each interface or belong to each union. Without this, cache reads for interface/union fields return `null` or incomplete data.

### Configuration

Generate `possibleTypes` at the **project level** (for project-specific schemas) or **schema_types level** (for shared schemas):

```yaml
# graphox.yaml
projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
    output_dir: "__generated__"
    possible_types: "graphql-introspection.ts"

schema_types:
  - schema: "schema.graphql"
    output: "types/schema.ts"
    possible_types: "types/possible-types.ts"
```

### Generated Output

```typescript
// graphql-introspection.ts
/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export interface PossibleTypesResultData {
  possibleTypes: { [key: string]: string[] }
}

const result: PossibleTypesResultData = {
  possibleTypes: {
    "Node": ["Album", "Artist", "Track"],
    "SearchResult": ["Album", "Artist", "Track"],
    "Displayable": ["Album", "Track"]
  },
};

export default result;
```

### Usage with Apollo Client

```typescript
// apollo/index.ts
import { ApolloClient, InMemoryCache } from '@apollo/client';
import possibleTypes from '../graphql-introspection';

const cache = new InMemoryCache({
  possibleTypes: possibleTypes.possibleTypes,
});

const client = new ApolloClient({
  cache,
  // ... other options
});
```

### Multi-Schema Projects

For projects with multiple schemas, generate `possibleTypes` at the **project level** for each project:

```yaml
# graphox.yaml
projects:
  # Business API project
  - schema: "schemas/business.graphql"
    include: "apps/business/src/**/*.{ts,tsx}"
    possible_types: "apps/business/src/graphql-introspection.ts"

  # Storefront project (different schema)
  - schema: "schemas/storefront.graphql"
    include: "apps/storefront/src/**/*.{ts,tsx}"
    possible_types: "apps/storefront/src/graphql-introspection.ts"
```

### Single Shared Schema

For multiple projects sharing the same schema, generate `possibleTypes` once at the **schema_types level**:

```yaml
# graphox.yaml
schema_types:
  - schema: "shared/schema.graphql"
    output: "shared/schema.types.ts"
    possible_types: "shared/possible-types.ts"

projects:
  - schema: "shared/schema.graphql"
    include: "apps/business/**/*.{ts,tsx}"

  - schema: "shared/schema.graphql"
    include: "apps/storefront/**/*.{ts,tsx}"
```

---

## Apollo Client TypePolicies

Generate strict TypeScript types for Apollo Client `TypePolicies` configuration. This provides compile-time safety for your cache configuration.

### Overview

Apollo Client's `TypePolicies` can be error-prone when typed generically. This feature generates strictly typed `FieldPolicy`, `KeySpecifier`, and `TypePolicy` types for every object and interface in your schema.

### Configuration

```yaml
# graphox.yaml
projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
    output_dir: "__generated__"
    type_policies: "type-policies.ts"

schema_types:
  - schema: "schema.graphql"
    output: "types/schema.ts"
    type_policies: "types/type-policies.ts"
```

### Generated Output

```typescript
// type-policies.ts
import { FieldPolicy, FieldReadFunction, TypePolicies, TypePolicy } from '@apollo/client/cache';

export type UserKeySpecifier = ('id' | 'name' | 'email' | UserKeySpecifier)[];

export type UserFieldPolicy = {
  id?: FieldPolicy<any> | FieldReadFunction<any>,
  name?: FieldPolicy<any> | FieldReadFunction<any>,
  email?: FieldPolicy<any> | FieldReadFunction<any>,
};

export type StrictTypedTypePolicies = {
  User?: Omit<TypePolicy, "fields" | "keyFields"> & {
    keyFields?: false | UserKeySpecifier | (() => undefined | UserKeySpecifier),
    fields?: UserFieldPolicy,
  },
  // ... other types
};

export type TypedTypePolicies = StrictTypedTypePolicies & TypePolicies;
```

### Usage with Apollo Client

```typescript
// apollo/index.ts
import { TypedTypePolicies } from '../type-policies';
import { InMemoryCache } from '@apollo/client/cache';

const cache = new InMemoryCache({
  typePolicies: {
    User: {
      keyFields: ['id'],
      fields: {
        friends: {
          merge: (existing, incoming) => [...incoming],
        },
      },
    },
  } as TypedTypePolicies,
});
```

### Combining with possibleTypes

Generate both features together. You can generate them to separate files or the same file:

```yaml
# graphox.yaml
projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
    possible_types: "graphql-introspection.ts"  # Separate files
    type_policies: "type-policies.ts"

  # Or generate to the same file:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
    possible_types: "apollo-shared.ts"
    type_policies: "apollo-shared.ts"  # Same path = concatenated output
```

When both are set to the same path, their outputs are concatenated with proper formatting.

---

## Selective Codegen

Disable codegen for specific projects or enable it only where needed.

```yaml
# graphox.yaml
projects:
  # Shared fragments - only parse, don't generate types
  - schema: "packages/shared/schema.graphql"
    include: "packages/shared/fragments/**/*.graphql"
    codegen: false

  # API server - full codegen
  - schema: "packages/api/schema.graphql"
    include: "packages/api/src/**/*.{ts,tsx}"
    emit_permission_data: true
    output_dir: "__generated__"

  # Web client - full codegen
  - schema: "packages/api/schema.graphql"  # Reuse API schema
    include: "packages/web/src/**/*.{ts,tsx}"
    output_dir: "__generated__"
```

---

## Ignoring Deprecations

Suppress warnings for deprecated fields or types.

```yaml
# graphox.yaml
ignore_deprecations:
  - "EXPERIMENTAL"
  - "INTERNAL"
  - "UseNewFieldInstead"

projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
    output_dir: "__generated__"
```

---

## Performance Tuning

Configure performance-related settings for large workspaces.

```yaml
# graphox.yaml
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
    output_dir: "__generated__"
```

---

## LSP Request Tracing

Debug slow LSP requests by enabling tracing.

```yaml
# graphox.yaml
tracing:
  enabled: true
  threshold_ms: 50  # Only trace requests exceeding 50ms

projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"
    output_dir: "__generated__"
```

Logs appear in the LSP output panel of your editor.

---

## Validation Rules

Enable additional validation rules for stricter checks.

```yaml
# graphox.yaml

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
    output_dir: "__generated__"
```

See [Validation Rules](./rules.md) for full documentation.
