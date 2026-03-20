# Validation Rules

graphox includes configurable validation rules that you can enable in `graphox.yaml`. All rules are errors that fail validation.

## Quick Reference

| Rule | Type | Default | Description |
|------|------|---------|-------------|
| `unique_operation_name` | `boolean` | `false` | Ensures operation names are unique |
| `no_duplicate_fields` | `boolean` | `false` | Detects duplicate fields in selection sets |
| `no_unused_fragments` | `boolean` | `false` | Detects unused fragment definitions |
| `required_fields` | `map` | `{}` | Ensures operations include required fields |
| `forbidden_fields` | `map` | `{}` | Ensures operations exclude forbidden fields |

## Enabling Rules

```yaml
# graphox.yaml
rules:
  unique_operation_name: true
  no_duplicate_fields: true
  no_unused_fragments: true
  required_fields:
    id: true
    permissions: ["mutation"]
```

---

## unique_operation_name

Ensures operation names are unique across the entire project.

**Enable:**
```yaml
rules:
  unique_operation_name: true
```

**Disallows:**
```graphql
# File: queries.graphql
query GetUser {
  user { id }
}

# File: other.graphql
query GetUser {  # Error: already defined in queries.graphql
  user { name }
}
```

---

## no_duplicate_fields

Detects duplicate fields in the same selection set by response key.

**Enable:**
```yaml
rules:
  no_duplicate_fields: true
```

**Disallows:**
```graphql
query GetUser {
  user {
    id
    id  # Error: duplicate field 'id'
    name
  }
}
```

---

## no_unused_fragments

Detects fragment definitions that are not used by any operation in the workspace.

**Enable:**
```yaml
rules:
  no_unused_fragments: true
```

**Disallows:**
```graphql
# File: fragments.graphql
fragment UnusedFragment on Query {  # Warning: unused fragment
  me { name }
}

# File: queries.graphql
query GetUser {
  me { id }
}
# UnusedFragment is never referenced
```

**Notes:**
- Fragments marked with `@type_only` directive are excluded from this check
- Fragments are considered "used" when referenced via the spread syntax (`...FragmentName`) in any operation across the workspace

---

## required_fields

Ensures operations include required fields if the types expose them. Each field can be required for all operations or specific operation types.

**Enable:**
```yaml
rules:
  required_fields:
    id: true               # Required in all operations
    permissions: ["query"] # Required only in queries
```

**Options per field:**
- `true` - Required in all operations (query, mutation, subscription)
- `false` - Disabled (field not required)
- `["query", "mutation", "subscription"]` - Required only in specified operation types

**Inline ignore comments:**

You can suppress a specific `required_fields` diagnostic by adding `# graphox-ignore` on the same line as the parent selection field.

```graphql
query GetUser {
  user { # graphox-ignore
    name
  }
}
```

**Disallows:**
```graphql
# With rule: required_fields: { id: true, requestId: true }

query GetUser {
  user {  # Error: missing required field 'id'
    name
  }
}

query GetUser {
  user {  # OK
    id
    name
  }
}
```

---

## forbidden_fields

Ensures operations do NOT include forbidden fields. Each field can be forbidden for all operations or specific operation types.

**Enable:**
```yaml
rules:
  forbidden_fields:
    password: true               # Forbidden in all operations
    internalNote: ["mutation"] # Forbidden only in mutations
```

**Options per field:**
- `true` - Forbidden in all operations (query, mutation, subscription)
- `false` - Disabled (field not forbidden)
- `["query", "mutation", "subscription"]` - Forbidden only in specified operation types

**Inline ignore comments:**

You can suppress a specific `forbidden_fields` diagnostic by adding `# graphox-ignore` on the same line as the forbidden field.

```graphql
query GetUser {
  user {
    password # graphox-ignore
  }
}
```

**Disallows:**
```graphql
# With rule: forbidden_fields: { password: true }

query GetUser {
  user {
    password  # Error: field 'password' is forbidden
  }
}
```

**Provides code action:** "Remove forbidden field" to automatically delete the field.

---

## Example Configuration

```yaml
# graphox.yaml
output_dir: "__generated__"

projects:
  - schema: "schema.graphql"
    include: "src/**/*.{ts,tsx}"

rules:
  unique_operation_name: true
  no_duplicate_fields: true
  no_unused_fragments: true
  required_fields:
    id: true
    createdAt: ["query", "mutation"]
```

---

## See Also

- [Ignoring Deprecations](./configurations.md#ignoring-deprecations)
- [Configuration](./configurations.md)
- [Architecture](./architecture.md)
