/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;
import type { EvaluationQuery, EvaluationQueryVariables } from "./query.codegen";
import type { PlaylistInfoQuery, PlaylistInfoQueryVariables } from "./query.codegen";
import type { PlaylistOwner } from "./query.codegen";
import type { UpdatePlaylistArchivedMutation, UpdatePlaylistArchivedMutationVariables } from "./mutation.codegen";
import { EvaluationQueryDocument } from "./query.codegen";
import { PlaylistInfoQueryDocument } from "./query.codegen";
import { UpdatePlaylistArchivedMutationDocument } from "./mutation.codegen";

const documents: { [key: string]: any } = {
  "fragment PlaylistOwner on User {\n  id\n  email\n}\n\nquery PlaylistInfo($id: ID!) {\n  playlist(id: $id) {\n    id\n    name\n    archived\n    owner {\n      ...PlaylistOwner\n    }\n  }\n}\n\nquery Evaluation($id: ID!) {\n  evaluation(id: $id) {\n    id\n    score\n  }\n}\n": EvaluationQueryDocument,
  "mutation UpdatePlaylistArchived($id: ID!, $archived: Boolean!) {\n  updatePlaylistArchived(id: $id, archived: $archived) {\n    id\n    archived\n    name\n  }\n}\n": UpdatePlaylistArchivedMutationDocument,
};

export function graphql(source: "fragment PlaylistOwner on User {\n  id\n  email\n}\n\nquery PlaylistInfo($id: ID!) {\n  playlist(id: $id) {\n    id\n    name\n    archived\n    owner {\n      ...PlaylistOwner\n    }\n  }\n}\n\nquery Evaluation($id: ID!) {\n  evaluation(id: $id) {\n    id\n    score\n  }\n}\n"): typeof EvaluationQueryDocument;
export function graphql(source: "mutation UpdatePlaylistArchived($id: ID!, $archived: Boolean!) {\n  updatePlaylistArchived(id: $id, archived: $archived) {\n    id\n    archived\n    name\n  }\n}\n"): typeof UpdatePlaylistArchivedMutationDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;

// Re-exports
export type { UpdatePlaylistArchivedMutation, UpdatePlaylistArchivedMutationHookResult, UpdatePlaylistArchivedMutationResult, UpdatePlaylistArchivedMutationVariables } from "./mutation.codegen";
export { UpdatePlaylistArchivedMutationDocument, useUpdatePlaylistArchivedMutation } from "./mutation.codegen";
export type { EvaluationLazyQueryHookResult, EvaluationQuery, EvaluationQueryHookResult, EvaluationQueryResult, EvaluationQueryVariables, PlaylistInfoLazyQueryHookResult, PlaylistInfoQuery, PlaylistInfoQueryHookResult, PlaylistInfoQueryResult, PlaylistInfoQueryVariables } from "./query.codegen";
export { EvaluationQueryDocument, PlaylistInfoQueryDocument, useEvaluationLazyQuery, useEvaluationQuery, usePlaylistInfoLazyQuery, usePlaylistInfoQuery } from "./query.codegen";
export type { PlaylistOwner } from "./query.codegen";
