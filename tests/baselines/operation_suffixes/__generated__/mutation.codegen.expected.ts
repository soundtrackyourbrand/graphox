/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface CreateUserMut {
  __typename: "Mutation";
  createUser: {
    __typename: "User";
    id: string;
    name: string;
  };
}

export type CreateUserMutVariables = Exact<{
  name: string;
}>;

export interface DeleteUserMut {
  __typename: "Mutation";
  deleteUser: boolean;
}

export type DeleteUserMutVariables = Exact<{
  id: string;
}>;
export const CreateUserMutDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"CreateUser"},"operation":"mutation","selectionSet":{"kind":"SelectionSet","selections":[{"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"name"},"value":{"kind":"Variable","name":{"kind":"Name","value":"name"}}}],"kind":"Field","name":{"kind":"Name","value":"createUser"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"name"}}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"String"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"name"}}}]}],"kind":"Document"} as unknown as DocumentNode<CreateUserMut, CreateUserMutVariables>;
export const DeleteUserMutDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"DeleteUser"},"operation":"mutation","selectionSet":{"kind":"SelectionSet","selections":[{"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"id"},"value":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}],"kind":"Field","name":{"kind":"Name","value":"deleteUser"}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}]}],"kind":"Document"} as unknown as DocumentNode<DeleteUserMut, DeleteUserMutVariables>;
