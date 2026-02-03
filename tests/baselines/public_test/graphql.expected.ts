/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import { GetPublicQuery, GetPublicQueryVariables, GetPublicQueryDocument } from "./pkg_b/query.codegen";

const documents: { [key: string]: any } = {
  "query GetPublic { users { ...PublicFrag } }\n": GetPublicQueryDocument,
};

export function graphql(source: "query GetPublic { users { ...PublicFrag } }\n"): typeof GetPublicQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
