import { graphql } from '../generated';

export const UserFragment = graphql(/* GraphQL */ `
  fragment UserFragment on User {
    id
    name
  }
`);
