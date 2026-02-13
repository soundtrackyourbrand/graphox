/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;
import type { create_postMutation, create_postMutationVariables } from "./query.codegen";
import type { get_all_postsQuery, get_all_postsQueryVariables } from "./query.codegen";
import type { get_user_by_idQuery, get_user_by_idQueryVariables } from "./user_query.codegen";
import type { user_fields } from "./query.codegen";
import { create_postMutationDocument } from "./query.codegen";
import { get_all_postsQueryDocument } from "./query.codegen";
import { get_user_by_idQueryDocument } from "./user_query.codegen";

const documents: { [key: string]: any } = {
  "query get_user_by_id($id: ID!) {\n  user(id: $id) {\n    id\n    name\n    email\n  }\n}\n": get_user_by_idQueryDocument,
  "query get_user_by_id($id: ID!) {\n  user(id: $id) {\n    id\n    name\n    email\n  }\n}\n\nquery get_all_posts {\n  posts {\n    id\n    title\n    content\n    author {\n      id\n      name\n    }\n  }\n}\n\nmutation create_post($title: String!, $content: String!) {\n  createPost(title: $title, content: $content) {\n    id\n    title\n    content\n  }\n}\n\nfragment user_fields on User {\n  id\n  name\n  email\n}\n": create_postMutationDocument,
};

export function graphql(source: "query get_user_by_id($id: ID!) {\n  user(id: $id) {\n    id\n    name\n    email\n  }\n}\n"): typeof get_user_by_idQueryDocument;
export function graphql(source: "query get_user_by_id($id: ID!) {\n  user(id: $id) {\n    id\n    name\n    email\n  }\n}\n\nquery get_all_posts {\n  posts {\n    id\n    title\n    content\n    author {\n      id\n      name\n    }\n  }\n}\n\nmutation create_post($title: String!, $content: String!) {\n  createPost(title: $title, content: $content) {\n    id\n    title\n    content\n  }\n}\n\nfragment user_fields on User {\n  id\n  name\n  email\n}\n"): typeof create_postMutationDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
