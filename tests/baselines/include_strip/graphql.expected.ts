/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import { GetUserQuery, GetUserQueryVariables, GetUserQueryDocument } from "./queries/user/get_user.codegen";
import { GetUserQuery, GetUserQueryVariables, GetUserQueryDocument } from "./test.codegen";

const documents: { [key: string]: any } = {
  "query GetUser($id: ID!) {\n  user(id: $id) {\n    ...UserFields\n  }\n}\n": GetUserQueryDocument,
  "\n  query GetUser($id: ID!) {\n    user(id: $id) {\n      ...UserFields\n    }\n  }\n": GetUserQueryDocument,
};

export function graphql(source: "query GetUser($id: ID!) {\n  user(id: $id) {\n    ...UserFields\n  }\n}\n"): typeof GetUserQueryDocument;
export function graphql(source: "\n  query GetUser($id: ID!) {\n    user(id: $id) {\n      ...UserFields\n    }\n  }\n"): typeof GetUserQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
