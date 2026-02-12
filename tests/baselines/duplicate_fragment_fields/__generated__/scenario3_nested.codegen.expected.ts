/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { FragmentType } from "./fragment-masking";
import { UserNestedA, UserNestedB } from "./fragments.codegen";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetUsersWithNestedFragmentsQuery {
  __typename: "Query";
  users: Array<({ __typename: "User" } & { ' $fragmentRefs'?: { 'UserNestedA': UserNestedA, 'UserNestedB': UserNestedB } })>;
}

export type GetUsersWithNestedFragmentsQueryVariables = Exact<{
}>;

export const GetUsersWithNestedFragmentsQueryDocument = {"definitions":[{"directives":[],"kind":"OperationDefinition","name":{"kind":"Name","value":"GetUsersWithNestedFragments"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"users"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null},{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"role"},"selectionSet":null},{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"email"},"selectionSet":null}]}}]},"variableDefinitions":[]},{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserContact"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"email"},"selectionSet":null}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}},{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserFullName"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}},{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserNestedA"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null},{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"role"},"selectionSet":null}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}},{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserNestedB"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null},{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"email"},"selectionSet":null}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}],"kind":"Document"} as unknown as DocumentNode<GetUsersWithNestedFragmentsQuery, GetUsersWithNestedFragmentsQueryVariables>;

