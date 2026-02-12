/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { PublicFrag } from "../pkg_a/fragment.codegen";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetPublicQuery {
  __typename: "Query";
  users: Array<({ __typename: "User" } & PublicFrag) | null> | null;
}

export type GetPublicQueryVariables = Exact<{
}>;

export const GetPublicQueryDocument = {"definitions":[{"directives":[],"kind":"OperationDefinition","name":{"kind":"Name","value":"GetPublic"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"users"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null}]}}]},"variableDefinitions":[]},{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"PublicFrag"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}],"kind":"Document"} as unknown as DocumentNode<GetPublicQuery, GetPublicQueryVariables>;

