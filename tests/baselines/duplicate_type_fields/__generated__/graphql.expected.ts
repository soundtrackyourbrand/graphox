/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { FragmentType } from "./fragment-masking";
import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { GetPlaylistQuery, GetPlaylistQueryVariables } from "./query.codegen";
import { GetPlaylistQueryDocument } from "./query.codegen";

const documents: { [key: string]: any } = {
  "query GetPlaylist($id: ID!) {\n  playlist(id: $id) {\n    ...DuplicateFields\n  }\n}\n": GetPlaylistQueryDocument,
};

export function graphql(source: "query GetPlaylist($id: ID!) {\n  playlist(id: $id) {\n    ...DuplicateFields\n  }\n}\n"): typeof GetPlaylistQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
