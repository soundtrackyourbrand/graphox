import { GetUsersWithComplexFilters, SearchEverything, CreateNewUser } from '@monorepo-e2e/app/src/main';

console.log('App starting...');

async function run() {
  console.log('Query 1:', GetUsersWithComplexFilters);
  console.log('Query 2:', SearchEverything);
  console.log('Mutation:', CreateNewUser);
}

run().catch(console.error);
