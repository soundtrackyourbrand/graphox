/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { FragmentType } from "./fragment-masking";
import { UserNestedA, UserNestedB } from "./fragments.codegen";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetUsersWithNestedFragmentsQuery {
  __typename: "Query";
  users: Array<Identity<({ __typename: "User" } & { ' $fragmentRefs'?: { 'UserNestedA': UserNestedA, 'UserNestedB': UserNestedB } })>>;
}

export type GetUsersWithNestedFragmentsQueryVariables = Exact<{
}>;
export const GetUsersWithNestedFragmentsQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"GetUsersWithNestedFragments"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"users"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"FragmentSpread","name":{"kind":"Name","value":"UserNestedA"}},{"kind":"FragmentSpread","name":{"kind":"Name","value":"UserNestedB"}}]}}]}},{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserContact"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"email"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}},{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserFullName"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"name"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}},{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserNestedA"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"FragmentSpread","name":{"kind":"Name","value":"UserFullName"}},{"kind":"Field","name":{"kind":"Name","value":"role"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}},{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserNestedB"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"FragmentSpread","name":{"kind":"Name","value":"UserFullName"}},{"kind":"FragmentSpread","name":{"kind":"Name","value":"UserContact"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}],"kind":"Document"} as unknown as DocumentNode<GetUsersWithNestedFragmentsQuery, GetUsersWithNestedFragmentsQueryVariables>;
