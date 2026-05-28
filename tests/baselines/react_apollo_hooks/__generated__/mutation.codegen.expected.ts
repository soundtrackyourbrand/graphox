/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import * as ApolloReactCommon from "@apollo/client/react";
import * as ApolloReactHooks from "@apollo/client/react";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

const defaultOptions = {} as const;

export interface UpdatePlaylistArchivedMutation {
  __typename: "Mutation";
  updatePlaylistArchived: {
    __typename: "Playlist";
    id: string;
    archived: boolean;
    name: string;
  };
}

export type UpdatePlaylistArchivedMutationVariables = Exact<{
  id: string;
  archived: boolean;
}>;
export const UpdatePlaylistArchivedMutationDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"UpdatePlaylistArchived"},"operation":"mutation","selectionSet":{"kind":"SelectionSet","selections":[{"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"id"},"value":{"kind":"Variable","name":{"kind":"Name","value":"id"}}},{"kind":"Argument","name":{"kind":"Name","value":"archived"},"value":{"kind":"Variable","name":{"kind":"Name","value":"archived"}}}],"kind":"Field","name":{"kind":"Name","value":"updatePlaylistArchived"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"archived"}},{"kind":"Field","name":{"kind":"Name","value":"name"}}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"id"}}},{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"Boolean"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"archived"}}}]}],"kind":"Document"} as unknown as DocumentNode<UpdatePlaylistArchivedMutation, UpdatePlaylistArchivedMutationVariables>;

export type UpdatePlaylistArchivedMutationHookResult = ReturnType<typeof useUpdatePlaylistArchivedMutation>;
export type UpdatePlaylistArchivedMutationResult = ApolloReactCommon.MutationResult<UpdatePlaylistArchivedMutation>;

export function useUpdatePlaylistArchivedMutation(baseOptions?: ApolloReactHooks.MutationHookOptions<UpdatePlaylistArchivedMutation, UpdatePlaylistArchivedMutationVariables>) {
  const options = { ...defaultOptions, ...baseOptions };
  return ApolloReactHooks.useMutation<UpdatePlaylistArchivedMutation, UpdatePlaylistArchivedMutationVariables>(UpdatePlaylistArchivedMutationDocument, options);
}
