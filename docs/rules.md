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

Ensures operations include required fields if the types expose them. Each field can be required for all operations or specific operation types. You can also restrict rules to specific GraphQL types.

**Enable:**
```yaml
rules:
  required_fields:
    id: true               # Required on ALL types in all operations
    permissions: ["query"] # Required on ALL types only in queries

    User:                  # Type-specific rules
      email: true          # Required only on User type
      name:
        enabled: ["query"]
        reason: "Names are required for display"
```

**Options per field:**
- `true` - Required in all operations (query, mutation, subscription)
- `false` - Disabled (field not required). If used inside a type namespace, it overrides a global rule.
- `["query", "mutation", "subscription"]` - Required only in specified operation types
- `{ enabled: ..., reason: "..." }` - Specify rule and a reason that appears in diagnostics

**Resolution Order:**
1. Type-specific rule (e.g., `User: { email: true }`)
2. Global rule (e.g., `email: true`)
3. If neither exists, the field is not required.

**Inline ignore comments:**

You can suppress a specific `required_fields` diagnostic by adding `# graphox-ignore` on the same line as the parent selection field.

```graphql
query GetUser {
  user { # graphox-ignore
    name
  }
}
```

**Fragments:**

The rule applies to every object a fragment reaches, at every depth, evaluated per operation that spreads it:

```graphql
# With rule: required_fields: { permissions: ["query"] }

fragment ZoneFields on SoundZone {
  id
  device {
    id            # Error in the query below: missing required field 'permissions'
  }
}
```

Because the operation type decides the rule, the diagnostic lands on the spread rather than inside the fragment, and names the fragment the object lives in. Suppress it with `# graphox-ignore` on the spread. Rules that hold for every operation type (`id: true`) are reported inside the fragment definition instead.

A path a fragment nests and the same path selected inline are one selection set, and are checked merged: `zone { device { permissions } ...ZoneFields }` satisfies the rule even though neither side selects everything on its own.

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

Ensures operations do NOT include forbidden fields. Each field can be forbidden for all operations or specific operation types. You can also restrict rules to specific GraphQL types.

**Enable:**
```yaml
rules:
  forbidden_fields:
    password: true               # Forbidden on ALL types in all operations
    internalNote: ["mutation"]   # Forbidden on ALL types only in mutations
    
    User:                        # Type-specific rules
      name: true                 # Forbidden only on User type
      email: false               # Explicitly allowed on User (overrides global)
      auditLog:
        enabled: true
        reason: "Use the dedicated audit system"
```

**Options per field:**
- `true` - Forbidden in all operations (query, mutation, subscription)
- `false` - Disabled (field not forbidden). If used inside a type namespace, it overrides a global rule.
- `["query", "mutation", "subscription"]` - Forbidden only in specified operation types
- `{ enabled: ..., reason: "..." }` - Specify rule and a reason that appears in diagnostics

**Resolution Order:**
1. Type-specific rule (e.g., `User: { name: true }`)
2. Global rule (e.g., `name: true`)
3. If neither exists, the field is not forbidden.

**Inline ignore comments:**

You can suppress a specific `forbidden_fields` diagnostic by adding `# graphox-ignore` on the same line as the parent selection field (similar to `required_fields`).

```graphql
query GetUser {
  user { # graphox-ignore
    password
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

**Fragments:**

Fields selected by a fragment count as selections of every operation that spreads it, so the rule is evaluated against the operation's type. The diagnostic lands on the spread and names the fragment:

```graphql
# With rule: forbidden_fields: { permissions: ["subscription"] }

fragment ZoneFields on SoundZone {
  id
  permissions
}

query GetZone($id: ID!) {
  soundZone(id: $id) {
    ...ZoneFields  # OK, the rule only covers subscriptions
  }
}

subscription OnZoneUpdate($input: SoundZoneUpdateInput!) {
  soundZoneUpdate(input: $input) {
    soundZone {
      ...ZoneFields  # Error: field 'permissions' is forbidden ... selected via fragment 'ZoneFields'
    }
  }
}
```

Since the fragment may be shared by operations that are allowed to select the field, these diagnostics offer only the ignore action, not the removal one. Suppress them with `# graphox-ignore` on the parent selection field, as above.

Objects nested inside a fragment body are checked the same way, at every depth and through chains of spreads:

```graphql
fragment ZoneFields on SoundZone {
  id
  device {
    id
    permissions   # forbidden in the subscription below, required in the query
  }
}
```

The diagnostic lands on the spread that reaches the object, since that is where the operation type is known, and names the fragment the selection lives in. Suppress these with `# graphox-ignore` on the spread itself, which is where they point.

A path a fragment nests and the same path selected inline are one selection set, and the rules see them merged — `zone { device { permissions } ...ZoneFields }` satisfies a rule that either side alone would not. When the document selects the path itself, the diagnostic stays on that selection and does not name a fragment.

Rules that hold for every operation type (`password: true`) are reported inside the fragment definition instead, where the selection is, rather than once per spread.

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
