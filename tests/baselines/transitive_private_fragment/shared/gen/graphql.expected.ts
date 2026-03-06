/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;
import { PrivateFragmentDocument } from "./fragments.codegen";
import { PublicFragmentDocument } from "./fragments.codegen";

const documents: { [key: string]: any } = {
  "\n  fragment PrivateFragment on Profile {\n    bio\n  }\n": PrivateFragmentDocument,
  "\n  fragment PublicFragment on User @public {\n    id\n    name\n    profile {\n      ...PrivateFragment\n    }\n  }\n": PublicFragmentDocument,
};

export function graphql(source: "\n  fragment PrivateFragment on Profile {\n    bio\n  }\n"): typeof PrivateFragmentDocument;
export function graphql(source: "\n  fragment PublicFragment on User @public {\n    id\n    name\n    profile {\n      ...PrivateFragment\n    }\n  }\n"): typeof PublicFragmentDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
