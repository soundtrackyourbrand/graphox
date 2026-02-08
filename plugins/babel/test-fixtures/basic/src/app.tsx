import { graphql } from './graphql';

export const query = graphql(`query GetUser {
  user(id: "1") {
    id
    name
  }
}`);
