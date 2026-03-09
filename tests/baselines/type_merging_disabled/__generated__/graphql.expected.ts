/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;
import type { SearchQueryQuery, SearchQueryQueryVariables } from "./query.codegen";
import { SearchQueryQueryDocument } from "./query.codegen";

const documents: { [key: string]: any } = {
  "query SearchQuery {\n  search {\n    ... on Artist {\n      id\n      name\n    }\n    ... on Album {\n      id\n      title\n    }\n    ... on Playlist {\n      id\n      title\n    }\n    ... on Track {\n      id\n      title\n    }\n  }\n}\n": SearchQueryQueryDocument,
};

export function graphql(source: "query SearchQuery {\n  search {\n    ... on Artist {\n      id\n      name\n    }\n    ... on Album {\n      id\n      title\n    }\n    ... on Playlist {\n      id\n      title\n    }\n    ... on Track {\n      id\n      title\n    }\n  }\n}\n"): typeof SearchQueryQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
