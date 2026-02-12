/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { FragmentType } from "./fragment-masking";
import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;
export type Incremental<T> = T | { [P in keyof T]?: P extends ' $fragmentName' | '__typename' ? T[P] : never };
import type { DuplicateFields } from "./fragments.codegen";
import type { GetPlaylistQuery, GetPlaylistQueryVariables } from "./query.codegen";
import { GetPlaylistQueryDocument } from "./query.codegen";

const documents: { [key: string]: any } = {
  "query GetPlaylist($id: ID!) {\n  playlist(id: $id) {\n    ...DuplicateFields\n  }\n}\n": GetPlaylistQueryDocument,
  "fragment DuplicateFields on Playlist @public {\n  id\n  __typename\n  permissions\n  presentAs\n  name\n  snapshot\n  shortDescription\n  curated\n  curator {\n    id\n    accountId\n    name\n  }\n  presets {\n    playbackMode\n  }\n  snapshot\n  permissions\n  presentAs\n}\n": {},
};

export function graphql(source: "query GetPlaylist($id: ID!) {\n  playlist(id: $id) {\n    ...DuplicateFields\n  }\n}\n"): typeof GetPlaylistQueryDocument;
export function graphql(source: "fragment DuplicateFields on Playlist @public {\n  id\n  __typename\n  permissions\n  presentAs\n  name\n  snapshot\n  shortDescription\n  curated\n  curator {\n    id\n    accountId\n    name\n  }\n  presets {\n    playbackMode\n  }\n  snapshot\n  permissions\n  presentAs\n}\n"): DocumentNode<DuplicateFields, unknown>;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
