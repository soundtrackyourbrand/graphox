/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

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

export interface CreateUserMutVariables {
  name: string;
}

export const CreateUserMutDocument = {"definitions":[{"directives":[],"kind":"OperationDefinition","name":{"kind":"Name","value":"CreateUser"},"operation":"mutation","selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"name"},"value":{"kind":"Variable","name":{"kind":"Name","value":"name"}}}],"directives":[],"kind":"Field","name":{"kind":"Name","value":"createUser"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null}]}}]},"variableDefinitions":[{"defaultValue":null,"directives":[],"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"String"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"name"}}}]}],"kind":"Document"} as unknown as DocumentNode<CreateUserMut, Exact<CreateUserMutVariables>>;

export interface DeleteUserMut {
  __typename: "Mutation";
  deleteUser: boolean;
}

export interface DeleteUserMutVariables {
  id: string;
}

export const DeleteUserMutDocument = {"definitions":[{"directives":[],"kind":"OperationDefinition","name":{"kind":"Name","value":"DeleteUser"},"operation":"mutation","selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"id"},"value":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}],"directives":[],"kind":"Field","name":{"kind":"Name","value":"deleteUser"},"selectionSet":null}]},"variableDefinitions":[{"defaultValue":null,"directives":[],"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}]}],"kind":"Document"} as unknown as DocumentNode<DeleteUserMut, Exact<DeleteUserMutVariables>>;

