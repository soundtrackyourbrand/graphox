/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;
import type { GetUserQuery, GetUserQueryVariables } from "./query.codegen";
import type { UserFieldsFrag } from "./fragment.codegen";
import { GetUserQueryDocument } from "./query.codegen";

const documents: { [key: string]: any } = {
  "query GetUser($id: ID!) {\n  user(id: $id) {\n    ...UserFields\n  }\n}\n": GetUserQueryDocument,
  "fragment UserFields on User {\n  id\n  name\n}\n": {},
};

export function graphql(source: "query GetUser($id: ID!) {\n  user(id: $id) {\n    ...UserFields\n  }\n}\n"): typeof GetUserQueryDocument;
export function graphql(source: "fragment UserFields on User {\n  id\n  name\n}\n"): DocumentNode<UserFieldsFrag, unknown>;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
