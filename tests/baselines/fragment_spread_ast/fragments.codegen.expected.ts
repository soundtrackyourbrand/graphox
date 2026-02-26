/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface AppQueryQuery {
  __typename: "Query";
  user: ({ __typename: "User" } & FragA) | null;
}

export type AppQueryQueryVariables = Exact<{
  id: string;
}>;

export interface FragC {
  __typename: "User";
  id: string;
}

export type FragB = ({ __typename: "User", name: string | null } & FragC);

export type FragA = Identity<({ __typename: "User", email: string | null } & (FragB & FragC))>;
export const FragCDocument = { kind: 'Document', definitions: [{"kind":"FragmentDefinition","name":{"kind":"Name","value":"FragC"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}] } as unknown as DocumentNode<FragC, unknown>;
export const FragBDocument = { kind: 'Document', definitions: [{"kind":"FragmentDefinition","name":{"kind":"Name","value":"FragB"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"name"}},{"kind":"FragmentSpread","name":{"kind":"Name","value":"FragC"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}, FragCDocument.definitions[0]] } as unknown as DocumentNode<FragB, unknown>;
export const FragADocument = { kind: 'Document', definitions: [{"kind":"FragmentDefinition","name":{"kind":"Name","value":"FragA"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"email"}},{"kind":"FragmentSpread","name":{"kind":"Name","value":"FragB"}},{"kind":"FragmentSpread","name":{"kind":"Name","value":"FragC"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}, FragBDocument.definitions[0], FragCDocument.definitions[0]] } as unknown as DocumentNode<FragA, unknown>;
export const AppQueryQueryDocument = { kind: 'Document', definitions: [{"kind":"OperationDefinition","name":{"kind":"Name","value":"AppQuery"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"id"},"value":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}],"kind":"Field","name":{"kind":"Name","value":"user"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"FragmentSpread","name":{"kind":"Name","value":"FragA"}}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}]}, FragADocument.definitions[0], FragBDocument.definitions[0], FragCDocument.definitions[0]] } as unknown as DocumentNode<AppQueryQuery, AppQueryQueryVariables>;
