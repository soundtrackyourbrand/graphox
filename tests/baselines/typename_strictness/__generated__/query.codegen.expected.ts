/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface TestQueryQuery {
  __typename: "Query";
  user: ({ __typename: "User" } & UserIdOnly) | null;
}

export type TestQueryQueryVariables = Exact<{
}>;

export interface UserIdOnly {
  __typename: "User";
  id: string;
}
export const TestQueryQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"TestQuery"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"user"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"FragmentSpread","name":{"kind":"Name","value":"UserIdOnly"}}]}}]}},{"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserIdOnly"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}],"kind":"Document"} as unknown as DocumentNode<TestQueryQuery, TestQueryQueryVariables>;
