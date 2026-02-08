import { gql } from './graphql';

export const query = gql(`query GetMe {
  me {
    id
    name
  }
}`);
