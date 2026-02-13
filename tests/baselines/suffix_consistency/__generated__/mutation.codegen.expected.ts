/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface UpdateUser {
  __typename: "Mutation";
  updateUser: {
    __typename: "User";
    id: string;
    name: string | null;
  } | null;
}

export type UpdateUserVariables = Exact<{
  name: string;
}>;

export const UpdateUserDoc = { kind: 'Document', definitions: [{"kind":"OperationDefinition","name":{"kind":"Name","value":"UpdateUser"},"operation":"mutation","selectionSet":{"kind":"SelectionSet","selections":[{"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"name"},"value":{"kind":"Variable","name":{"kind":"Name","value":"name"}}}],"kind":"Field","name":{"kind":"Name","value":"updateUser"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"name"}}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"String"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"name"}}}]}] } as unknown as DocumentNode<UpdateUser, UpdateUserVariables>;

