/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;
import type { AddTracks_CreateManualPlaylistMutation, AddTracks_CreateManualPlaylistMutationVariables } from "./query.codegen";
import type { ChangePlan_AccountQuery, ChangePlan_AccountQueryVariables } from "./query.codegen";
import type { ChangePlan_PricesQuery, ChangePlan_PricesQueryVariables } from "./query.codegen";
import type { CreatePostMutation, CreatePostMutationVariables } from "./query.codegen";
import type { GenerateOtpQuery, GenerateOtpQueryVariables } from "./query.codegen";
import type { GetAllPostsQuery, GetAllPostsQueryVariables } from "./query.codegen";
import type { GetSamlConfigQuery, GetSamlConfigQueryVariables } from "./query.codegen";
import type { GetUserByIdQuery, GetUserByIdQueryVariables } from "./user_query.codegen";
import type { UserFields } from "./query.codegen";
import { AddTracks_CreateManualPlaylistMutationDocument } from "./query.codegen";
import { ChangePlan_AccountQueryDocument } from "./query.codegen";
import { ChangePlan_PricesQueryDocument } from "./query.codegen";
import { CreatePostMutationDocument } from "./query.codegen";
import { GenerateOtpQueryDocument } from "./query.codegen";
import { GetAllPostsQueryDocument } from "./query.codegen";
import { GetSamlConfigQueryDocument } from "./query.codegen";
import { GetUserByIdQueryDocument } from "./user_query.codegen";

const documents: { [key: string]: any } = {
  "query get_user_by_id($id: ID!) {\n  user(id: $id) {\n    id\n    name\n    email\n  }\n}\n": GetUserByIdQueryDocument,
  "query get_user_by_id($id: ID!) {\n  user(id: $id) {\n    id\n    name\n    email\n  }\n}\n\nquery get_all_posts {\n  posts {\n    id\n    title\n    content\n    author {\n      id\n      name\n    }\n  }\n}\n\nmutation create_post($title: String!, $content: String!) {\n  createPost(title: $title, content: $content) {\n    id\n    title\n    content\n  }\n}\n\nfragment user_fields on User {\n  id\n  name\n  email\n}\n\n# Edge case: acronym at start\nquery generateOTP($input: GenerateOTPInput!) {\n  generateOTP(input: $input) {\n    otp\n    expiresAt\n  }\n}\n\n# Edge case: acronym in middle  \nquery getSAMLConfig($accountId: ID!) {\n  samlConfig(id: $accountId) {\n    expiresAt\n    slug\n  }\n}\n\n# Edge case: underscore between camelCase segments\nmutation AddTracks_CreateManualPlaylist($input: CreateManualPlaylistInput!) {\n  createManualPlaylist(input: $input) {\n    id\n    permissions\n  }\n}\n\n# Edge case: underscore between uppercase and lowercase segments\nquery ChangePlan_account($accountId: ID!) {\n  account(id: $accountId) {\n    id\n    name\n  }\n}\n\nquery ChangePlan_prices {\n  prices {\n    id\n    amount\n  }\n}\n": AddTracks_CreateManualPlaylistMutationDocument,
};

export function graphql(source: "query get_user_by_id($id: ID!) {\n  user(id: $id) {\n    id\n    name\n    email\n  }\n}\n"): typeof GetUserByIdQueryDocument;
export function graphql(source: "query get_user_by_id($id: ID!) {\n  user(id: $id) {\n    id\n    name\n    email\n  }\n}\n\nquery get_all_posts {\n  posts {\n    id\n    title\n    content\n    author {\n      id\n      name\n    }\n  }\n}\n\nmutation create_post($title: String!, $content: String!) {\n  createPost(title: $title, content: $content) {\n    id\n    title\n    content\n  }\n}\n\nfragment user_fields on User {\n  id\n  name\n  email\n}\n\n# Edge case: acronym at start\nquery generateOTP($input: GenerateOTPInput!) {\n  generateOTP(input: $input) {\n    otp\n    expiresAt\n  }\n}\n\n# Edge case: acronym in middle  \nquery getSAMLConfig($accountId: ID!) {\n  samlConfig(id: $accountId) {\n    expiresAt\n    slug\n  }\n}\n\n# Edge case: underscore between camelCase segments\nmutation AddTracks_CreateManualPlaylist($input: CreateManualPlaylistInput!) {\n  createManualPlaylist(input: $input) {\n    id\n    permissions\n  }\n}\n\n# Edge case: underscore between uppercase and lowercase segments\nquery ChangePlan_account($accountId: ID!) {\n  account(id: $accountId) {\n    id\n    name\n  }\n}\n\nquery ChangePlan_prices {\n  prices {\n    id\n    amount\n  }\n}\n"): typeof AddTracks_CreateManualPlaylistMutationDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
