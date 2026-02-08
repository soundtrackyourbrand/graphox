import { graphql } from './graphql';

export const meQuery = graphql(`query GetMe {
  me {
    id
    name
  }
}`);

export const userQuery = graphql(`query GetUser($id: ID!) {
  user(id: $id) {
    id
    name
    email
  }
}`);
