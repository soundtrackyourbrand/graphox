/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { PrivateFrag, PublicFrag } from "../pkg_a/fragment.codegen";

export interface GetPublicQuery {
  __typename: "Query";
  users: Array<({ __typename: "User" } & PublicFrag) | null> | null;
}

export const GetPublicDocument = {"definitions":[{"directives":[],"kind":"OperationDefinition","name":{"kind":"Name","value":"GetPublic"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"users"},"selectionSet":{"kind":"SelectionSet","selections":[{"directives":[],"kind":"FragmentSpread","name":{"kind":"Name","value":"PublicFrag"}}]}}]},"variableDefinitions":[]},{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"PublicFrag"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}],"kind":"Document"} as unknown as DocumentNode<GetPublicQuery, { [key: string]: never; }>;

export interface GetPrivateQuery {
  __typename: "Query";
  users: Array<({ __typename: "User" } & PrivateFrag) | null> | null;
}

export const GetPrivateDocument = {"definitions":[{"directives":[],"kind":"OperationDefinition","name":{"kind":"Name","value":"GetPrivate"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"users"},"selectionSet":{"kind":"SelectionSet","selections":[{"directives":[],"kind":"FragmentSpread","name":{"kind":"Name","value":"PrivateFrag"}}]}}]},"variableDefinitions":[]},{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"PrivateFrag"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}],"kind":"Document"} as unknown as DocumentNode<GetPrivateQuery, { [key: string]: never; }>;

