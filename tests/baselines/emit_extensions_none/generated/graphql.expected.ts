/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;
import type { GetMeQuery, GetMeQueryVariables } from "./queries.codegen";
import type { MyFragment } from "./App.codegen";
import { GetMeQueryDocument } from "./queries.codegen";

const documents: { [key: string]: any } = {
  "\n  query GetMe {\n    me {\n      id\n      name\n    }\n  }\n": GetMeQueryDocument,
  "\n  fragment MyFragment on User {\n    id\n    name\n  }\n": {},
};

export function graphql(source: "\n  query GetMe {\n    me {\n      id\n      name\n    }\n  }\n"): typeof GetMeQueryDocument;
export function graphql(source: "\n  fragment MyFragment on User {\n    id\n    name\n  }\n"): DocumentNode<MyFragment, unknown>;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
