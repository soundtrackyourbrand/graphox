/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { MyEnum } from "@my/schema";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface Query1Query {
  __typename: "Query";
  test: string | null;
}

export type Query1QueryVariables = Exact<{
  e?: MyEnum | null;
}>;

export const Query1QueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"Query1"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"test"},"selectionSet":null}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NamedType","name":{"kind":"Name","value":"MyEnum"}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"e"}}}]}],"kind":"Document"} as unknown as DocumentNode<Query1Query, Query1QueryVariables>;

