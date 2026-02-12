/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { GetMeQuery, GetMeQueryVariables } from "./query.codegen";
import type { GetMyIdQuery, GetMyIdQueryVariables } from "./subdir/other.codegen";
import { GetMeQueryDocument } from "./query.codegen";
import { GetMyIdQueryDocument } from "./subdir/other.codegen";

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
