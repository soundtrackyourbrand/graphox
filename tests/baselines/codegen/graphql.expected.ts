/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import { GetNodePolymorphicQuery, GetNodePolymorphicQueryVariables, GetNodePolymorphicQueryDocument } from "./union_query.codegen";
import { GetNodeQuery, GetNodeQueryVariables, GetNodeQueryDocument } from "./query_with_variables.codegen";
import { GetUsersQuery, GetUsersQueryVariables, GetUsersQueryDocument } from "./simple_query.codegen";
import { GetUsersWithFragmentQuery, GetUsersWithFragmentQueryVariables, GetUsersWithFragmentQueryDocument } from "./fragment_usage.codegen";

const documents: { [key: string]: any } = {
  "query GetUsersWithFragment {\n  users {\n    ...UserFields\n    email\n  }\n}\n": GetUsersWithFragmentQueryDocument,
  "query GetNode($id: ID!) {\n  node(id: $id) {\n    id\n    ... on User {\n      username\n    }\n  }\n}\n": GetNodeQueryDocument,
  "query GetUsers {\n  users {\n    id\n    username\n  }\n}\n": GetUsersQueryDocument,
  "query GetNodePolymorphic($id: ID!) {\n  node(id: $id) {\n    __typename\n    ... on User {\n      id\n      username\n    }\n    ... on Post {\n      id\n      title\n    }\n  }\n}\n": GetNodePolymorphicQueryDocument,
};

export function graphql(source: "query GetUsersWithFragment {\n  users {\n    ...UserFields\n    email\n  }\n}\n"): typeof GetUsersWithFragmentQueryDocument;
export function graphql(source: "query GetNode($id: ID!) {\n  node(id: $id) {\n    id\n    ... on User {\n      username\n    }\n  }\n}\n"): typeof GetNodeQueryDocument;
export function graphql(source: "query GetUsers {\n  users {\n    id\n    username\n  }\n}\n"): typeof GetUsersQueryDocument;
export function graphql(source: "query GetNodePolymorphic($id: ID!) {\n  node(id: $id) {\n    __typename\n    ... on User {\n      id\n      username\n    }\n    ... on Post {\n      id\n      title\n    }\n  }\n}\n"): typeof GetNodePolymorphicQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
