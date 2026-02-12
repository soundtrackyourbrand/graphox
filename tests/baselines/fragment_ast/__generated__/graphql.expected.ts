/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { GetMeQuery, GetMeQueryVariables } from "./query.codegen";
import { GetMeQueryDocument } from "./query.codegen";
import { UserFieldsDocument } from "./fragment.codegen";

const documents: { [key: string]: any } = {
  "query GetMe { me { ...UserFields } }\n": GetMeQueryDocument,
  "fragment UserFields on User @public { id name }\n": UserFieldsDocument,
};

export function graphql(source: "query GetMe { me { ...UserFields } }\n"): typeof GetMeQueryDocument;
export function graphql(source: "fragment UserFields on User @public { id name }\n"): typeof UserFieldsDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
