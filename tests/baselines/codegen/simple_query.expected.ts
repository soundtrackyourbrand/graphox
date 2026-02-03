/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

export interface GetUsersQuery {
  __typename: "Query";
  users: Array<{
    __typename: "User";
    id: string;
    username: string;
  } | null> | null;
}

export type GetUsersDocument = DocumentNode<GetUsersQuery, { [key: string]: never; }>;

