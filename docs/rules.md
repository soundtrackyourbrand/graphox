# Validation Rules

graphql-rust includes configurable validation rules that you can enable in `graphql.yaml`. All rules are errors that fail validation.

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

## required_fields

Ensures operations include required fields. Each field can be required for all operations or specific operation types.

**Enable:**
```yaml
rules:
  required_fields:
    id: true               # Required in all operations for types that has an id property
    permissions: ["query"] # Required only in queries
```

**Options per field:**
- `true` - Required in all operations (query, mutation, subscription)
- `false` - Disabled (field not required)
- `["query", "mutation", "subscription"]` - Required only in specified operation types

**Disallows:**
```graphql
# With rule: required_fields: { requestId: true }

query GetUser {
  user {  # Error: missing required field 'id'
    name
    permissions
  }
}

query GetUser {
  user {  # OK
    id
    name
    permissions
  }
}
```
