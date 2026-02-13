/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;
import type { TestQueryQuery, TestQueryQueryVariables } from "./query.codegen";
import type { UserIdOnly } from "./query.codegen";
import { TestQueryQueryDocument } from "./query.codegen";

const documents: { [key: string]: any } = {
  "\n  fragment UserIdOnly on User {\n    id\n  }\n\n  query TestQuery {\n    user {\n      ...UserIdOnly\n    }\n  }\n": TestQueryQueryDocument,
};

export function graphql(source: "\n  fragment UserIdOnly on User {\n    id\n  }\n\n  query TestQuery {\n    user {\n      ...UserIdOnly\n    }\n  }\n"): typeof TestQueryQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
