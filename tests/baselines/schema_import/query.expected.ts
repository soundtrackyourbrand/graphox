/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from '@graphql-typed-document-node/core';
import type { Role } from "@workspace/graphql-schema";

export interface GetMeQuery {
  __typename: "Query";
  me: {
    __typename: "User";
    id: string;
    role: Role;
  } | null;
}

export type GetMeDocument = DocumentNode<GetMeQuery, { [key: string]: never; }>;
