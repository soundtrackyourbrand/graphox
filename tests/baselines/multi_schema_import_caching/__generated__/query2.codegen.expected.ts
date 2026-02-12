/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { MyEnum } from "@my/schema";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface Query2Query {
  __typename: "Query";
  test: string | null;
}

export type Query2QueryVariables = Exact<{
  e?: MyEnum | null;
}>;

export const Query2QueryDocument = {"definitions":[{"directives":[],"kind":"OperationDefinition","name":{"kind":"Name","value":"Query2"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"e"},"value":{"kind":"Variable","name":{"kind":"Name","value":"e"}}}],"directives":[],"kind":"Field","name":{"kind":"Name","value":"test"},"selectionSet":null}]},"variableDefinitions":[{"defaultValue":null,"directives":[],"kind":"VariableDefinition","type":{"kind":"NamedType","name":{"kind":"Name","value":"MyEnum"}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"e"}}}]}],"kind":"Document"} as unknown as DocumentNode<Query2Query, Query2QueryVariables>;

