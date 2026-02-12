/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;
import type { GetMeQuery, GetMeQueryVariables } from "./query.codegen";
import { GetMeQueryDocument } from "./query.codegen";

const documents: { [key: string]: any } = {
  "query GetMe {\n  me {\n    id\n    username\n  }\n}\n": GetMeQueryDocument,
};

export function graphql(source: "query GetMe {\n  me {\n    id\n    username\n  }\n}\n"): typeof GetMeQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
