/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;
import type { AddressFields } from "./query.codegen";
import type { CreatePostMutation, CreatePostMutationVariables } from "./query.codegen";
import type { GetAllUsersQuery, GetAllUsersQueryVariables } from "./query.codegen";
import type { GetPostsWithFragmentQuery, GetPostsWithFragmentQueryVariables } from "./query.codegen";
import type { GetUserByIdQuery, GetUserByIdQueryVariables } from "./query.codegen";
import type { GetUsersWithAddressesQuery, GetUsersWithAddressesQueryVariables } from "./query.codegen";
import type { PostFields } from "./query.codegen";
import type { UserFields } from "./query.codegen";
import type { UserWithAddress } from "./query.codegen";
import { CreatePostMutationDocument } from "./query.codegen";
import { GetAllUsersQueryDocument } from "./query.codegen";
import { GetPostsWithFragmentQueryDocument } from "./query.codegen";
import { GetUserByIdQueryDocument } from "./query.codegen";
import { GetUsersWithAddressesQueryDocument } from "./query.codegen";

const documents: { [key: string]: any } = {
  "query get_user_by_id($id: ID!) {\n  user(id: $id) {\n    id\n    name\n    email\n  }\n}\n\nfragment user_fields on User {\n  id\n  name\n  email\n}\n\nquery get_all_users {\n  users {\n    ...user_fields\n  }\n}\n\nfragment post_fields on Post {\n  id\n  title\n  content\n}\n\nquery get_posts_with_fragment {\n  posts {\n    ...post_fields\n    author {\n      id\n      name\n    }\n  }\n}\n\nmutation create_post($title: String!, $content: String!) {\n  createPost(title: $title, content: $content) {\n    id\n    title\n    content\n  }\n}\n\n# Test nested fragments\nfragment address_fields on Address {\n  street\n  city\n  country\n}\n\nfragment user_with_address on User {\n  id\n  name\n  ...address_fields\n}\n\nquery get_users_with_addresses {\n  users {\n    ...user_with_address\n  }\n}\n": CreatePostMutationDocument,
};

export function graphql(source: "query get_user_by_id($id: ID!) {\n  user(id: $id) {\n    id\n    name\n    email\n  }\n}\n\nfragment user_fields on User {\n  id\n  name\n  email\n}\n\nquery get_all_users {\n  users {\n    ...user_fields\n  }\n}\n\nfragment post_fields on Post {\n  id\n  title\n  content\n}\n\nquery get_posts_with_fragment {\n  posts {\n    ...post_fields\n    author {\n      id\n      name\n    }\n  }\n}\n\nmutation create_post($title: String!, $content: String!) {\n  createPost(title: $title, content: $content) {\n    id\n    title\n    content\n  }\n}\n\n# Test nested fragments\nfragment address_fields on Address {\n  street\n  city\n  country\n}\n\nfragment user_with_address on User {\n  id\n  name\n  ...address_fields\n}\n\nquery get_users_with_addresses {\n  users {\n    ...user_with_address\n  }\n}\n"): typeof CreatePostMutationDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
