import { graphql } from "./gen/graphql";

const query = graphql(`query GetMe {
  me {
    id
    username
  }
}
`);

console.log(query);
