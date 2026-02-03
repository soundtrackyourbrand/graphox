/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

export interface GetNodePolymorphicQuery {
  __typename: "Query";
  node: { __typename: "Node" }
    | {
      __typename: "User";
      id: string;
      username: string;
    } | {
      __typename: "Post";
      id: string;
      title: string;
    } | null;
}

export interface GetNodePolymorphicQueryVariables {
  id: string;
}

export type GetNodePolymorphicDocument = DocumentNode<GetNodePolymorphicQuery, GetNodePolymorphicQueryVariables>;

