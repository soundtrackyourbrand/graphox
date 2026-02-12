/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { GetMeQuery, GetMeQueryVariables } from "./graphql.codegen.ts";
import { GetMeQueryDocument } from "./graphql.codegen.ts";

const documents: { [key: string]: any } = {
  "\n  query GetMe {\n    me {\n      id\n      name\n    }\n  }\n": GetMeQueryDocument,
};

export function graphql(source: "\n  query GetMe {\n    me {\n      id\n      name\n    }\n  }\n"): typeof GetMeQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
