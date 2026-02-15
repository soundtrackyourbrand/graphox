/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetNonNullUsersQuery {
  __typename: "Query";
  nonNullUsers: Array<{
    __typename: "User";
    id: string;
    username: string;
  }>;
}

export type GetNonNullUsersQueryVariables = Exact<{
}>;
export const GetNonNullUsersQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"GetNonNullUsers"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"nonNullUsers"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"username"}}]}}]}}],"kind":"Document"} as unknown as DocumentNode<GetNonNullUsersQuery, GetNonNullUsersQueryVariables>;
