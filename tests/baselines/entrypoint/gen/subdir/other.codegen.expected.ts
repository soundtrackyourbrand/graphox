/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetMyIdQuery {
  __typename: "Query";
  me: {
    __typename: "User";
    id: string;
  } | null;
}

export type GetMyIdQueryVariables = Exact<{
}>;

export const GetMyIdQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"GetMyId"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"me"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null}]}}]}}],"kind":"Document"} as unknown as DocumentNode<GetMyIdQuery, GetMyIdQueryVariables>;

