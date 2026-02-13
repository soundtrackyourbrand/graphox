/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { PublicFrag } from "../pkg_a/fragment.codegen";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetPublicQuery {
  __typename: "Query";
  users: Array<({ __typename: "User" } & PublicFrag) | null> | null;
}

export type GetPublicQueryVariables = Exact<{
}>;

export const GetPublicQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"GetPublic"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"users"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"FragmentSpread","name":{"kind":"Name","value":"PublicFrag"}}]}}]}},{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"PublicFrag"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}],"kind":"Document"} as unknown as DocumentNode<GetPublicQuery, GetPublicQueryVariables>;

