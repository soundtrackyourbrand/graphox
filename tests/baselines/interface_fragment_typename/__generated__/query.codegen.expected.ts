/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface TestQueryQuery {
  __typename: "Query";
  items: Array<({
      __typename: "Schedule";
    } & DisplayableInfo)
    | ({
      __typename: "Curator";
    } & DisplayableInfo)>;
}

export type TestQueryQueryVariables = Exact<{
}>;

export const TestQueryQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"TestQuery"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"items"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"FragmentSpread","name":{"kind":"Name","value":"DisplayableInfo"}}]}}]}},{"kind":"FragmentDefinition","name":{"kind":"Name","value":"DisplayableInfo"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"display"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"title"}}]}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"Displayable"}}}],"kind":"Document"} as unknown as DocumentNode<TestQueryQuery, TestQueryQueryVariables>;

export interface DisplayableInfo {
  __typename: "Schedule" | "Curator";
  display: {
    __typename: "Display";
    title: string;
  } | null;
}

