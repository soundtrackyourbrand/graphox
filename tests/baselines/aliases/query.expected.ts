/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

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

export interface GetComplexUserQueryVariables {
  id: string;
}

export const GetComplexUserDocument = {"definitions":[{"directives":[],"kind":"OperationDefinition","name":{"kind":"Name","value":"GetComplexUser"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"alias":{"kind":"Name","value":"currentUser"},"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"me"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":{"kind":"Name","value":"uid"},"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"alias":{"kind":"Name","value":"name"},"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"username"},"selectionSet":null}]}},{"alias":{"kind":"Name","value":"otherUser"},"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"id"},"value":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}],"directives":[],"kind":"Field","name":{"kind":"Name","value":"user"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"alias":{"kind":"Name","value":"alias1"},"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"username"},"selectionSet":null},{"alias":{"kind":"Name","value":"alias2"},"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"username"},"selectionSet":null},{"alias":{"kind":"Name","value":"connections"},"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"friends"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":{"kind":"Name","value":"friendId"},"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"alias":{"kind":"Name","value":"friendName"},"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"username"},"selectionSet":null}]}}]}}]},"variableDefinitions":[{"defaultValue":null,"directives":[],"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}]}],"kind":"Document"} as unknown as DocumentNode<GetComplexUserQuery, GetComplexUserQueryVariables>;

