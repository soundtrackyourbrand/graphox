/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetSettingsQuery {
  __typename: "Query";
  settings: {
    __typename: "Settings";
    theme: string;
    notifications: boolean;
  } | null;
}

export type GetSettingsQueryVariables = Exact<{
}>;

export const GetSettingsQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"GetSettings"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"settings"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"theme"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"notifications"},"selectionSet":null}]}}]}}],"kind":"Document"} as unknown as DocumentNode<GetSettingsQuery, GetSettingsQueryVariables>;

