import { graphql } from './__generated__';
export * from './__generated__';

const MyQuery = graphql(`
  query MyQuery {
    me {
      id
      name
    }
  }
`);

export { MyQuery };
