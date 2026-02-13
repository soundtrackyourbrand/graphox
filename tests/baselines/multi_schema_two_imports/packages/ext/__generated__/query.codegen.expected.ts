/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { Priority, UserStatus } from "@my/ext-package";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetUserWithTaskQuery {
  __typename: "Query";
  me: {
    __typename: "User";
    id: string;
    name: string;
    status: UserStatus;
  } | null;
  task: {
    __typename: "Task";
    id: string;
    title: string;
    priority: Priority;
  } | null;
}

export type GetUserWithTaskQueryVariables = Exact<{
}>;

export const GetUserWithTaskQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"GetUserWithTask"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"me"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"status"},"selectionSet":null}]}},{"kind":"Field","name":{"kind":"Name","value":"task"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"title"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"priority"},"selectionSet":null}]}}]}}],"kind":"Document"} as unknown as DocumentNode<GetUserWithTaskQuery, GetUserWithTaskQueryVariables>;

