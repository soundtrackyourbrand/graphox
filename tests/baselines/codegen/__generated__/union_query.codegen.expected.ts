/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetNodePolymorphicQuery {
  __typename: "Query";
  node: {
      __typename: "User";
      id: string;
      username: string;
    }
    | {
      __typename: "Post";
      id: string;
      title: string;
    } | {
      __typename: "Comment";
    } | null;
}

export type GetNodePolymorphicQueryVariables = Exact<{
  id: string;
}>;
export const GetNodePolymorphicQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"GetNodePolymorphic"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"id"},"value":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}],"kind":"Field","name":{"kind":"Name","value":"node"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"__typename"}},{"kind":"InlineFragment","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"username"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}},{"kind":"InlineFragment","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"title"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"Post"}}}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}]}],"kind":"Document"} as unknown as DocumentNode<GetNodePolymorphicQuery, GetNodePolymorphicQueryVariables>;
