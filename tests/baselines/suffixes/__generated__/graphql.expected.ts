/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { GetUserQuery, GetUserQueryVars } from "./query.codegen";
import type { UserFieldsFrag } from "./fragment.codegen";
import { GetUserQueryGQL } from "./query.codegen";

const documents: { [key: string]: any } = {
  "query GetUser($id: ID!) {\n  user(id: $id) {\n    ...UserFields\n    email\n  }\n}\n": GetUserQueryGQL,
  "fragment UserFields on User @public {\n  id\n  name\n}\n": {},
};

export function graphql(source: "query GetUser($id: ID!) {\n  user(id: $id) {\n    ...UserFields\n    email\n  }\n}\n"): typeof GetUserQueryGQL;
export function graphql(source: "fragment UserFields on User @public {\n  id\n  name\n}\n"): DocumentNode<UserFieldsFrag, unknown>;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
