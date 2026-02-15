import { graphql } from './__generated__/graphql';

// Simple inline query with variables and aliases
const UserCard = graphql(`
  query UserCard($id: ID!) {
    user(id: $id) {
      id
      name
      userName: name
      email
      role
      metadata
      createdAt
    }
  }
`);

// Inline query with directives
const UserCardWithOptionalEmail = graphql(`
  query UserCardWithOptionalEmail($id: ID!, $includeEmail: Boolean!) {
    user(id: $id) {
      id
      name
      email @include(if: $includeEmail)
      role
    }
  }
`);

// Export for type checking
export { UserCard, UserCardWithOptionalEmail };
