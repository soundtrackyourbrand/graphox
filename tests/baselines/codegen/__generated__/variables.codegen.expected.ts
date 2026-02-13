/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetUserByIdQuery {
  __typename: "Query";
  node: {
      __typename: "User";
      id: string;
      username: string;
    }
    | {
      __typename: "Post";
    } | {
      __typename: "Comment";
    } | null;
}

export type GetUserByIdQueryVariables = Exact<{
  id: string;
  tags: Array<string>;
}>;

export const GetUserByIdQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"GetUserById"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"node"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"InlineFragment","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"username"},"selectionSet":null}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"id"}}},{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"ListType","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"String"}}}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"tags"}}}]}],"kind":"Document"} as unknown as DocumentNode<GetUserByIdQuery, GetUserByIdQueryVariables>;

