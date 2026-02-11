/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import { Query1Query, Query1QueryVariables, Query1QueryDocument } from "./query1.codegen";
import { Query2Query, Query2QueryVariables, Query2QueryDocument } from "./query2.codegen";

const documents: { [key: string]: any } = {
  "query Query1($e: MyEnum) {\n  test(e: $e)\n}\n": Query1QueryDocument,
  "query Query2($e: MyEnum) {\n  test(e: $e)\n}\n": Query2QueryDocument,
};

export function graphql(source: "query Query1($e: MyEnum) {\n  test(e: $e)\n}\n"): typeof Query1QueryDocument;
export function graphql(source: "query Query2($e: MyEnum) {\n  test(e: $e)\n}\n"): typeof Query2QueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
