/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { UserFields } from "./fragments/user_fields.codegen";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetUserQuery {
  __typename: "Query";
  user: ({ __typename: "User" } & UserFields) | null;
}

export type GetUserQueryVariables = Exact<{
  id: string;
}>;

export const GetUserQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"GetUser"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"user"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"email"},"selectionSet":null}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}]},{"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserFields"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"email"},"selectionSet":null}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}],"kind":"Document"} as unknown as DocumentNode<GetUserQuery, GetUserQueryVariables>;

