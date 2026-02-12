import { graphql } from "./generated/graphql";

const fragment = graphql(`
  fragment MyFragment on User {
    id
    name
  }
`);
