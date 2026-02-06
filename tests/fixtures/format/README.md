# Format Test Fixtures and Baselines

This directory contains test fixtures for GraphQL formatting code actions.

## Structure

- **fixtures/format/**: TypeScript/TSX files with unformatted inline GraphQL
- **baselines/format/**: Expected formatted GraphQL output

## Test Files

### cramped_query.ts
Input: Cramped query without spacing
```typescript
const q = gql`query{me{id name email}}`;
```
Expected output: Properly formatted with indentation and line breaks

### cramped_mutation.tsx
Input: Cramped mutation with variables
```typescript
const mutation = gql`mutation UpdateUser($id:ID!,$name:String!){updateUser(id:$id,name:$name){id name}}`;
```
Expected output: Formatted mutation with proper spacing

### fragment_spread.ts
Input: Multiple GraphQL blocks (fragment + query) in one file
```typescript
const fragment = gql`fragment UserFields on User{id name email}`;
const query = gql`query{me{...UserFields}}`;
```
Expected output: Two separately formatted blocks

## Running Tests

```bash
cargo test format
```

## Updating Baselines

When the formatter output changes intentionally, update baselines by:

1. Manually verifying the new output is correct
2. Updating the `.expected.graphql` files in `tests/baselines/format/`
3. Running tests to confirm: `cargo test format`
