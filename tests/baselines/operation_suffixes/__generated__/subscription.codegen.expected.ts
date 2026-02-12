/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface OnUserCreatedSub {
  __typename: "Subscription";
  userCreated: {
    __typename: "User";
    id: string;
    name: string;
  };
}

export type OnUserCreatedSubVariables = Exact<{
}>;

export const OnUserCreatedSubDocument = {"definitions":[{"directives":[],"kind":"OperationDefinition","name":{"kind":"Name","value":"OnUserCreated"},"operation":"subscription","selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"userCreated"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null}]}}]},"variableDefinitions":[]}],"kind":"Document"} as unknown as DocumentNode<OnUserCreatedSub, OnUserCreatedSubVariables>;

