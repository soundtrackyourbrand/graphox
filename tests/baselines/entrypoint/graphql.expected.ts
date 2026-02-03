/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import { GetMeQuery, GetMeQueryVariables, GetMeQueryDocument } from "./query.codegen";
import { GetMyIdQuery, GetMyIdQueryVariables, GetMyIdQueryDocument } from "./subdir/other.codegen";

const documents: { [key: string]: any } = {
  "query GetMe {\n  me {\n    id\n    username\n  }\n}\n": GetMeQueryDocument,
  "query GetMyId {\n  me {\n    id\n  }\n}\n": GetMyIdQueryDocument,
};

export function graphql(source: "query GetMe {\n  me {\n    id\n    username\n  }\n}\n"): typeof GetMeQueryDocument;
export function graphql(source: "query GetMyId {\n  me {\n    id\n  }\n}\n"): typeof GetMyIdQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
