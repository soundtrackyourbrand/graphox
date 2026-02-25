/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;
import type { GetPlaybackQuery, GetPlaybackQueryVariables } from "./query.codegen";
import { GetPlaybackQueryDocument } from "./query.codegen";

const documents: { [key: string]: any } = {
  "query GetPlayback {\n  playback {\n    id\n    playable {\n      __typename\n      ... on Node {\n        id\n      }\n      ... on Displayable {\n        title\n      }\n      ... on Track {\n        durationMs\n      }\n    }\n  }\n}\n": GetPlaybackQueryDocument,
};

export function graphql(source: "query GetPlayback {\n  playback {\n    id\n    playable {\n      __typename\n      ... on Node {\n        id\n      }\n      ... on Displayable {\n        title\n      }\n      ... on Track {\n        durationMs\n      }\n    }\n  }\n}\n"): typeof GetPlaybackQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
