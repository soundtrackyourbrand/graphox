/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import * as ApolloReactCommon from "@apollo/client/react";
import * as ApolloReactHooks from "@apollo/client/react";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

const defaultOptions = {} as const;

export interface PlaylistInfoQuery {
  __typename: "Query";
  playlist: {
    __typename: "Playlist";
    id: string;
    name: string;
    archived: boolean;
    owner: ({ __typename: "User" } & PlaylistOwner);
  } | null;
}

export type PlaylistInfoQueryVariables = Exact<{
  id: string;
}>;

export interface EvaluationQuery {
  __typename: "Query";
  evaluation: {
    __typename: "Evaluation";
    id: string;
    score: number;
  } | null;
}

export type EvaluationQueryVariables = Exact<{
  id: string;
}>;

export interface PlaylistOwner {
  __typename: "User";
  id: string;
  email: string;
}
export const EvaluationQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"Evaluation"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"id"},"value":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}],"kind":"Field","name":{"kind":"Name","value":"evaluation"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"score"}}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}]}],"kind":"Document"} as unknown as DocumentNode<EvaluationQuery, EvaluationQueryVariables>;

export type EvaluationQueryHookResult = ReturnType<typeof useEvaluationQuery>;
export type EvaluationLazyQueryHookResult = ReturnType<typeof useEvaluationLazyQuery>;
export type EvaluationQueryResult = ApolloReactCommon.QueryResult<EvaluationQuery, EvaluationQueryVariables>;

export function useEvaluationQuery(baseOptions: ApolloReactHooks.QueryHookOptions<EvaluationQuery, EvaluationQueryVariables>) {
  const options = { ...defaultOptions, ...baseOptions };
  return ApolloReactHooks.useQuery<EvaluationQuery, EvaluationQueryVariables>(EvaluationQueryDocument, options);
}

export function useEvaluationLazyQuery(baseOptions?: ApolloReactHooks.LazyQueryHookOptions<EvaluationQuery, EvaluationQueryVariables>) {
  const options = { ...defaultOptions, ...baseOptions };
  return ApolloReactHooks.useLazyQuery<EvaluationQuery, EvaluationQueryVariables>(EvaluationQueryDocument, options);
}
export const PlaylistInfoQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"PlaylistInfo"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"id"},"value":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}],"kind":"Field","name":{"kind":"Name","value":"playlist"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"name"}},{"kind":"Field","name":{"kind":"Name","value":"archived"}},{"kind":"Field","name":{"kind":"Name","value":"owner"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"FragmentSpread","name":{"kind":"Name","value":"PlaylistOwner"}}]}}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}]},{"kind":"FragmentDefinition","name":{"kind":"Name","value":"PlaylistOwner"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"email"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}],"kind":"Document"} as unknown as DocumentNode<PlaylistInfoQuery, PlaylistInfoQueryVariables>;

export type PlaylistInfoQueryHookResult = ReturnType<typeof usePlaylistInfoQuery>;
export type PlaylistInfoLazyQueryHookResult = ReturnType<typeof usePlaylistInfoLazyQuery>;
export type PlaylistInfoQueryResult = ApolloReactCommon.QueryResult<PlaylistInfoQuery, PlaylistInfoQueryVariables>;

export function usePlaylistInfoQuery(baseOptions: ApolloReactHooks.QueryHookOptions<PlaylistInfoQuery, PlaylistInfoQueryVariables>) {
  const options = { ...defaultOptions, ...baseOptions };
  return ApolloReactHooks.useQuery<PlaylistInfoQuery, PlaylistInfoQueryVariables>(PlaylistInfoQueryDocument, options);
}

export function usePlaylistInfoLazyQuery(baseOptions?: ApolloReactHooks.LazyQueryHookOptions<PlaylistInfoQuery, PlaylistInfoQueryVariables>) {
  const options = { ...defaultOptions, ...baseOptions };
  return ApolloReactHooks.useLazyQuery<PlaylistInfoQuery, PlaylistInfoQueryVariables>(PlaylistInfoQueryDocument, options);
}
