/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from '@graphql-typed-document-node/core';

export interface GetNodeQuery {
  __typename: "Query";
  node: { __typename: "Node" }
    | {
      __typename: "User";
      username: string;
    } | null;
}

export interface GetNodeQueryVariables {
  id: string;
}

export type GetNodeDocument = DocumentNode<GetNodeQuery, GetNodeQueryVariables>;

