/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetComplexUserQuery {
  __typename: "Query";
  currentUser: {
    __typename: "User";
    uid: string;
    name: string;
  } | null;
  otherUser: {
    __typename: "User";
    id: string;
    alias1: string;
    alias2: string;
    connections: Array<{
      __typename: "User";
      friendId: string;
      friendName: string;
    } | null> | null;
  } | null;
}

export type GetComplexUserQueryVariables = Exact<{
  id: string;
}>;
export const GetComplexUserQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"GetComplexUser"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"alias":{"kind":"Name","value":"currentUser"},"kind":"Field","name":{"kind":"Name","value":"me"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":{"kind":"Name","value":"uid"},"kind":"Field","name":{"kind":"Name","value":"id"}},{"alias":{"kind":"Name","value":"name"},"kind":"Field","name":{"kind":"Name","value":"username"}}]}},{"alias":{"kind":"Name","value":"otherUser"},"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"id"},"value":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}],"kind":"Field","name":{"kind":"Name","value":"user"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"alias":{"kind":"Name","value":"alias1"},"kind":"Field","name":{"kind":"Name","value":"username"}},{"alias":{"kind":"Name","value":"alias2"},"kind":"Field","name":{"kind":"Name","value":"username"}},{"alias":{"kind":"Name","value":"connections"},"kind":"Field","name":{"kind":"Name","value":"friends"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":{"kind":"Name","value":"friendId"},"kind":"Field","name":{"kind":"Name","value":"id"}},{"alias":{"kind":"Name","value":"friendName"},"kind":"Field","name":{"kind":"Name","value":"username"}}]}}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}]}],"kind":"Document"} as unknown as DocumentNode<GetComplexUserQuery, GetComplexUserQueryVariables>;
