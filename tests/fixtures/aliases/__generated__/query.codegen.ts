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

export const GetComplexUserQueryDocument = {"definitions":[{"operation":"query","name":{"value":"GetComplexUser","kind":"Name"},"selectionSet":{"selections":[{"selectionSet":{"selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"alias":{"kind":"Name","value":"uid"}},{"alias":{"value":"name","kind":"Name"},"name":{"value":"username","kind":"Name"},"kind":"Field"}],"kind":"SelectionSet"},"alias":{"value":"currentUser","kind":"Name"},"kind":"Field","name":{"value":"me","kind":"Name"}},{"selectionSet":{"selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"username"},"alias":{"value":"alias1","kind":"Name"}},{"alias":{"value":"alias2","kind":"Name"},"kind":"Field","name":{"kind":"Name","value":"username"}},{"alias":{"kind":"Name","value":"connections"},"kind":"Field","name":{"value":"friends","kind":"Name"},"selectionSet":{"selections":[{"alias":{"value":"friendId","kind":"Name"},"name":{"kind":"Name","value":"id"},"kind":"Field"},{"kind":"Field","name":{"value":"username","kind":"Name"},"alias":{"kind":"Name","value":"friendName"}}],"kind":"SelectionSet"}}],"kind":"SelectionSet"},"alias":{"value":"otherUser","kind":"Name"},"name":{"value":"user","kind":"Name"},"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"id"},"value":{"kind":"Variable","name":{"value":"id","kind":"Name"}}}],"kind":"Field"}],"kind":"SelectionSet"},"variableDefinitions":[{"type":{"type":{"name":{"kind":"Name","value":"ID"},"kind":"NamedType"},"kind":"NonNullType"},"kind":"VariableDefinition","variable":{"name":{"kind":"Name","value":"id"},"kind":"Variable"}}],"kind":"OperationDefinition"}],"kind":"Document"} as unknown as DocumentNode<GetComplexUserQuery, GetComplexUserQueryVariables>;

