import { graphql } from './generated/graphql';

const query = graphql(`
  query GetUser($id: ID!) {
    user(id: $id) {
      ...UserFields
    }
  }
`);
