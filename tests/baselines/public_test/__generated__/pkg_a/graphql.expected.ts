/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { PublicFrag } from "./fragment.codegen";

const documents: { [key: string]: any } = {
  "fragment PublicFrag on User @public { id }\nfragment PrivateFrag on User { id }\n": {},
};

export function graphql(source: "fragment PublicFrag on User @public { id }\nfragment PrivateFrag on User { id }\n"): DocumentNode<PublicFrag, unknown>;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
