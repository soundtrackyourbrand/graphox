/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from '@graphql-typed-document-node/core';
import type { UserFields } from "./fragment_definition.graphql";

export interface GetUsersWithFragmentQuery {
  __typename: "Query";
  users: Array<({ __typename: "User", email: string | null } & UserFields) | null> | null;
}

export type GetUsersWithFragmentDocument = DocumentNode<GetUsersWithFragmentQuery, { [key: string]: never; }>;

