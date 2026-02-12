/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { GetNodePolymorphicQuery, GetNodePolymorphicQueryVariables } from "./union_query.codegen";
import type { GetNodeQuery, GetNodeQueryVariables } from "./query_with_variables.codegen";
import type { GetUsersQuery, GetUsersQueryVariables } from "./simple_query.codegen";
import type { GetUsersWithFragmentQuery, GetUsersWithFragmentQueryVariables } from "./fragment_usage.codegen";
import { GetNodePolymorphicQueryDocument } from "./union_query.codegen";
import { GetNodeQueryDocument } from "./query_with_variables.codegen";
import { GetUsersQueryDocument } from "./simple_query.codegen";
import { GetUsersWithFragmentQueryDocument } from "./fragment_usage.codegen";

const documents: { [key: string]: any } = {
  "query GetNodePolymorphic($id: ID!) {\n  node(id: $id) {\n    __typename\n    ... on User {\n      id\n      username\n    }\n    ... on Post {\n      id\n      title\n    }\n  }\n}\n": GetNodePolymorphicQueryDocument,
  "query GetNode($id: ID!) {\n  node(id: $id) {\n    id\n    ... on User {\n      username\n    }\n  }\n}\n": GetNodeQueryDocument,
  "query GetUsers {\n  users {\n    id\n    username\n  }\n}\n": GetUsersQueryDocument,
  "query GetUsersWithFragment {\n  users {\n    ...UserFields\n    email\n  }\n}\n": GetUsersWithFragmentQueryDocument,
};

export function graphql(source: "query GetNodePolymorphic($id: ID!) {\n  node(id: $id) {\n    __typename\n    ... on User {\n      id\n      username\n    }\n    ... on Post {\n      id\n      title\n    }\n  }\n}\n"): typeof GetNodePolymorphicQueryDocument;
export function graphql(source: "query GetNode($id: ID!) {\n  node(id: $id) {\n    id\n    ... on User {\n      username\n    }\n  }\n}\n"): typeof GetNodeQueryDocument;
export function graphql(source: "query GetUsers {\n  users {\n    id\n    username\n  }\n}\n"): typeof GetUsersQueryDocument;
export function graphql(source: "query GetUsersWithFragment {\n  users {\n    ...UserFields\n    email\n  }\n}\n"): typeof GetUsersWithFragmentQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
