/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { FragmentType } from "./fragment-masking";
import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import { GetUserQuery, GetUserQueryVariables, GetUserQueryDocument } from "./queries.codegen";
import { GetUserWithPostsQuery, GetUserWithPostsQueryVariables, GetUserWithPostsQueryDocument } from "./queries.codegen";
import { GetUsersQuery, GetUsersQueryVariables, GetUsersQueryDocument } from "./queries.codegen";

const documents: { [key: string]: any } = {
  "query GetUser($id: ID!) {\n  user(id: $id) {\n    ...UserFields\n  }\n}\n\nquery GetUsers {\n  users {\n    ...UserFields\n    ...UserEmail\n  }\n}\n\nquery GetUserWithPosts($id: ID!) {\n  user(id: $id) {\n    ...UserFields\n    posts {\n      ...UserPosts\n    }\n  }\n}\n": GetUserQueryDocument,
  "query GetUser($id: ID!) {\n  user(id: $id) {\n    ...UserFields\n  }\n}\n\nquery GetUsers {\n  users {\n    ...UserFields\n    ...UserEmail\n  }\n}\n\nquery GetUserWithPosts($id: ID!) {\n  user(id: $id) {\n    ...UserFields\n    posts {\n      ...UserPosts\n    }\n  }\n}\n": GetUsersQueryDocument,
  "query GetUser($id: ID!) {\n  user(id: $id) {\n    ...UserFields\n  }\n}\n\nquery GetUsers {\n  users {\n    ...UserFields\n    ...UserEmail\n  }\n}\n\nquery GetUserWithPosts($id: ID!) {\n  user(id: $id) {\n    ...UserFields\n    posts {\n      ...UserPosts\n    }\n  }\n}\n": GetUserWithPostsQueryDocument,
};

export function graphql(source: "query GetUser($id: ID!) {\n  user(id: $id) {\n    ...UserFields\n  }\n}\n\nquery GetUsers {\n  users {\n    ...UserFields\n    ...UserEmail\n  }\n}\n\nquery GetUserWithPosts($id: ID!) {\n  user(id: $id) {\n    ...UserFields\n    posts {\n      ...UserPosts\n    }\n  }\n}\n"): typeof GetUserQueryDocument;
export function graphql(source: "query GetUser($id: ID!) {\n  user(id: $id) {\n    ...UserFields\n  }\n}\n\nquery GetUsers {\n  users {\n    ...UserFields\n    ...UserEmail\n  }\n}\n\nquery GetUserWithPosts($id: ID!) {\n  user(id: $id) {\n    ...UserFields\n    posts {\n      ...UserPosts\n    }\n  }\n}\n"): typeof GetUsersQueryDocument;
export function graphql(source: "query GetUser($id: ID!) {\n  user(id: $id) {\n    ...UserFields\n  }\n}\n\nquery GetUsers {\n  users {\n    ...UserFields\n    ...UserEmail\n  }\n}\n\nquery GetUserWithPosts($id: ID!) {\n  user(id: $id) {\n    ...UserFields\n    posts {\n      ...UserPosts\n    }\n  }\n}\n"): typeof GetUserWithPostsQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
