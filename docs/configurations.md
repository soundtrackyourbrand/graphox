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
- [Apollo Client TypePolicies](#apollo-client-typepolicies)
- [Selective Codegen](#selective-codegen)
- [GraphQL Tag Fallback](#graphql-tag-fallback)
- [Nullable Fields as Optional](#nullable-fields-as-optional)
- [Ignoring Deprecations](#ignoring-deprecations)
- [Performance Tuning](#performance-tuning)
- [LSP Request Tracing](#lsp-request-tracing)
- [Empty Projects](#empty-projects)
- [Validation Rules](#validation-rules)

---

## Full Configuration Reference

```yaml
# Custom scalar type mappings
scalars:
  DateTime: "Date"
  JSON: "Record<string, any>"
  BigInt: "string"

# Ignore specific deprecation reasons in validation
ignore_deprecations:
  - "EXPERIMENTAL"
  - "INTERNAL"

# Codegen settings (can also be specified per-project)
codegen:
  # Generate and reuse AST nodes for fragments (for smaller bundle sizes)
  generate_ast_for_fragments: false
  
  # Type naming
  document_suffix: "Document"
  variables_suffix: "Variables"
  fragment_suffix: ""
  fragment_document_suffix: ""
  query_suffix: "Query"
  mutation_suffix: "Mutation"
  subscription_suffix: "Subscription"
  
  # Naming convention
  naming_convention: "pascal_case"  # or: "preserve"
  
  # Re-export types from graphql.ts
  re_exports: false

  # Delete generated files whose source document no longer exists
  prune_orphans: true

  # File extensions
  emit_extensions: "ts"  # or: "js", "tsx"

  # Development options
  # Use graphql-tag as a fallback for the generated graphql function
  graphql_tag_fallback: false

  # Type generation options
  # Generate nullable fields as optional properties (with '?')
  nullable_fields_as_optional: false

  # Fragment masking (similar to graphql-codegen client-preset)
  # Disabled by default for backwards compatibility
  fragment_masking: enabled  # or: disabled
  # fragment_masking:
  #   unmask_function_name: "getFragmentData"  # Custom function name

# Project configurations (required)
projects:
  - schema: "schema.graphql"                    # Single schema file
    include: "src/client/**/*.{ts,tsx}"         # Glob pattern(s)
    exclude: "**/*.test.ts"                      # Optional exclusions
    output_dir: "src/client/__generated__"       # Override global output_dir
    import: "@workspace/project-1"                # How other projects import fragments
    emit_permission_data: true                   # Generate permission metadata
    
    # Codegen settings for this project (overrides root)
    codegen:
      generate_ast_for_fragments: false
      document_suffix: "Document"
      variables_suffix: "Variables"
      fragment_suffix: ""
      fragment_masking: disabled
      naming_convention: "pascal_case"
      possible_types: "graphql-introspection.ts"
      type_policies: "type-policies.ts"

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

# Validation settings
allow_no_documents: false                         # Allow a project to match zero documents
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
codegen:
  fragment_masking: enabled

# Or with custom function name
codegen:
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
codegen:
  fragment_masking: enabled  # Global default

projects:
  # Uses global (enabled)
  - schema: "schema.graphql"
    include: "src/app/**/*"

  # Overrides to disabled
  - schema: "schema.graphql"
    include: "src/admin/**/*"
    codegen:
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

1. Enable fragment masking in config: `codegen.fragment_masking: enabled`
2. Update components to use `FragmentType<>` props
3. Replace direct field access with `getFragmentData()` calls

---

## Re-exports

Re-export all operation and fragment types/documents from the root `graphql.ts` file for easier imports.

### Configuration

```yaml
# Root level - applies to all projects
codegen:
  re_exports: true

# Or per-project
projects:
  - schema: "schema.graphql"
    include: "src/**/*"
    codegen:
      re_exports: true  # Override for this project
```

### Generated Output

With `re_exports: enabled`, the root `graphql.ts` file will re-export all types and documents:

```typescript
// graphql.ts
export type { GetUser, GetUserVariables } from "./query.codegen";
export { GetUserDocument } from "./query.codegen";
export type { UserFragment } from "./fragment.codegen";
```

This allows imports from a single file:

```typescript
import { GetUserDocument, type GetUserQuery, type UserFragment } from "./__generated__/graphql";
```

### Benefits

- **Single import point**: Import all types and documents from one file
- **Simplified imports**: No need to track which file contains which operation
- **Barrel file pattern**: Follows the common barrel file pattern for cleaner imports

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
    include: "apps/web/src/**/*.{ts,tsx}"
    possible_types: "apps/web/src/graphql-introspection.ts"

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
    include: "apps/web/**/*.{ts,tsx}"

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

## GraphQL Tag Fallback

Enable `graphql-tag` fallback for the generated `graphql` function. This is useful during development when you are adding new GraphQL literals that haven't been codegen yet.

### Configuration

```yaml
# graphox.yaml
codegen:
  graphql_tag_fallback: true
```

When enabled, the generated `graphql` function will use `gql` from `graphql-tag` as a fallback:

```typescript
import gqlTag from "graphql-tag";

// ...

export function graphql(source: string): any {
  return documents[source] || gqlTag(source);
}
```

This allows you to continue development without waiting for the codegen to finish or when you have operations that are not yet matched by any project configuration.

---

## Nullable Fields as Optional

Generate nullable GraphQL fields as optional properties in TypeScript interfaces using the `?` suffix.

### Configuration

```yaml
# graphox.yaml
codegen:
  nullable_fields_as_optional: true
```

**Without `nullable_fields_as_optional` (default):**

```typescript
export interface User {
  id: string;
  name: string | null;
}
```

**With `nullable_fields_as_optional: true`:**

```typescript
export interface User {
  id: string;
  name?: string | null;
}
```

---

## Ignoring Deprecations

Suppress warnings for deprecated fields or types. You can ignore deprecations globally via configuration or on a case-by-case basis using comments.

### Global Configuration

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

### Inline Comments

You can also ignore specific instances of deprecated fields by adding a `# graphox-ignore` comment on the same line as the field in your GraphQL operation.

```graphql
query GetUser {
  user {
    id
    deprecatedField # graphox-ignore
  }
}
```

This is particularly useful when you have a justified use of a deprecated field but want to maintain warnings for the rest of your codebase.

The same inline `# graphox-ignore` comment is also supported for `required_fields` and `forbidden_fields` diagnostics.

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

## Empty Projects

By default `graphox check` and `graphox codegen` fail if a project's
`documents` (or `include`) pattern matches no files. A project that collects
nothing produces no diagnostics and generates no output, so without this both
commands would exit 0 while silently having done nothing — usually because the
pattern is mistyped or points at a directory that has moved.

```yaml
# graphox.yaml
projects:
  - schema: "schema.graphql"
    documents: "src/**/*.{ts,tsx}"
```

```
Project 'srcc/**/*.{ts,tsx}' matched no documents. Fix the pattern, or set `allow_no_documents: true` to allow it.
Check failed.
```

Two cases are exempt, because neither is the mistake this catches: a project
with `codegen.enabled: false` during `graphox codegen`, and `graphox codegen
--clean`, which only removes generated files.

Set `allow_no_documents: true` to permit it — for a project that is
legitimately empty, such as one scaffolded ahead of the code that will fill
it.

```yaml
# graphox.yaml
allow_no_documents: true                # Global default for every project

projects:
  - schema: "schema.graphql"
    documents: "packages/new-app/**/*.ts"

  # Per-project settings override the global one, in either direction.
  - schema: "schema.graphql"
    documents: "packages/web/**/*.ts"
    allow_no_documents: false
```

Note that a leading `./` on a pattern is stripped, so `./src/**/*.ts` and
`src/**/*.ts` behave identically.

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
