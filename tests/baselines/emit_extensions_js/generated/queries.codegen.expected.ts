/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetMeQuery {
  __typename: "Query";
  me: {
    __typename: "User";
    id: string;
    name: string;
  } | null;
}

export type GetMeQueryVariables = Exact<{
}>;

export const GetMeQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"GetMe"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"me"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null}]}}]}}],"kind":"Document"} as unknown as DocumentNode<GetMeQuery, GetMeQueryVariables>;

