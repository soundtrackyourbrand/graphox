/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { UserFieldsFrag } from "./fragment.codegen";
import { UserFieldsDoc } from "./fragment.codegen";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetUserQuery {
  __typename: "Query";
  user: ({ __typename: "User" } & UserFieldsFrag) | null;
}

export type GetUserQueryVariables = Exact<{
}>;

export const GetUserQueryDoc = { kind: 'Document', definitions: [{"directives":[],"kind":"OperationDefinition","name":{"kind":"Name","value":"GetUser"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"user"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null}]}}]},"variableDefinitions":[]}, ...UserFieldsDoc.definitions] } as unknown as DocumentNode<GetUserQuery, GetUserQueryVariables>;

