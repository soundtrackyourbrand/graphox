import { GetUsersWithComplexFilters, SearchEverything, CreateNewUser } from '@monorepo-e2e/app/src/main';
import { graphql } from '#app/graphql/gql';

const AliasOnlyInRuntime = graphql(`
  query AliasOnlyInRuntime {
    __typename
  }
`);

console.log('App starting...');

async function run() {
  console.log('Query 1:', GetUsersWithComplexFilters);
  console.log('Query 2:', SearchEverything);
  console.log('Mutation:', CreateNewUser);
  console.log('Alias Query:', AliasOnlyInRuntime);
}

run().catch(console.error);
