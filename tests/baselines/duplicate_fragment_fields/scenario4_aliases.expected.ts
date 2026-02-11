/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { FragmentType } from "./fragment-masking";
import type { UserAliasedName, UserRealName } from "./fragments.codegen";

export interface GetUserWithAliasedFragmentsQuery {
  __typename: "Query";
  user: (FragmentType<typeof UserAliasedName> & FragmentType<typeof UserRealName>) | null;
}

export interface GetUserWithAliasedFragmentsQueryVariables {
  id: string;
}

export const GetUserWithAliasedFragmentsQueryDocument = {"definitions":[{"directives":[],"kind":"OperationDefinition","name":{"kind":"Name","value":"GetUserWithAliasedFragments"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"id"},"value":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}],"directives":[],"kind":"Field","name":{"kind":"Name","value":"user"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":{"kind":"Name","value":"userName"},"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null},{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null}]}}]},"variableDefinitions":[{"defaultValue":null,"directives":[],"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}]},{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserAliasedName"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":{"kind":"Name","value":"userName"},"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}},{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserRealName"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}],"kind":"Document"} as unknown as DocumentNode<GetUserWithAliasedFragmentsQuery, GetUserWithAliasedFragmentsQueryVariables>;

