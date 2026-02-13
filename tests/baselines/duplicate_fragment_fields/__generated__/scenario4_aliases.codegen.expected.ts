/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { FragmentType } from "./fragment-masking";
import { UserAliasedName, UserRealName } from "./fragments.codegen";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetUserWithAliasedFragmentsQuery {
  __typename: "Query";
  user: Identity<({ __typename: "User" } & { ' $fragmentRefs'?: { 'UserAliasedName': UserAliasedName, 'UserRealName': UserRealName } })> | null;
}

export type GetUserWithAliasedFragmentsQueryVariables = Exact<{
  id: string;
}>;

export const GetUserWithAliasedFragmentsQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"GetUserWithAliasedFragments"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"id"},"value":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}],"kind":"Field","name":{"kind":"Name","value":"user"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"FragmentSpread","name":{"kind":"Name","value":"UserAliasedName"}},{"kind":"FragmentSpread","name":{"kind":"Name","value":"UserRealName"}}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}]},{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserAliasedName"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":{"kind":"Name","value":"userName"},"kind":"Field","name":{"kind":"Name","value":"name"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}},{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserRealName"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"name"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}],"kind":"Document"} as unknown as DocumentNode<GetUserWithAliasedFragmentsQuery, GetUserWithAliasedFragmentsQueryVariables>;

