/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { MyEnum } from "@my/schema";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface Query1Query {
  __typename: "Query";
  test: string | null;
}

export interface Query1QueryVariables {
  e?: MyEnum | null;
}

export const Query1QueryDocument = {"definitions":[{"directives":[],"kind":"OperationDefinition","name":{"kind":"Name","value":"Query1"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"e"},"value":{"kind":"Variable","name":{"kind":"Name","value":"e"}}}],"directives":[],"kind":"Field","name":{"kind":"Name","value":"test"},"selectionSet":null}]},"variableDefinitions":[{"defaultValue":null,"directives":[],"kind":"VariableDefinition","type":{"kind":"NamedType","name":{"kind":"Name","value":"MyEnum"}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"e"}}}]}],"kind":"Document"} as unknown as DocumentNode<Query1Query, Exact<Query1QueryVariables>>;

