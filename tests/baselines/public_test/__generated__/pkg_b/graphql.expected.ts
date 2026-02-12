/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;
import type { GetPublicQuery, GetPublicQueryVariables } from "./query.codegen";
import { GetPublicQueryDocument } from "./query.codegen";

const documents: { [key: string]: any } = {
  "query GetPublic { users { ...PublicFrag } }\n": GetPublicQueryDocument,
};

export function graphql(source: "query GetPublic { users { ...PublicFrag } }\n"): typeof GetPublicQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
