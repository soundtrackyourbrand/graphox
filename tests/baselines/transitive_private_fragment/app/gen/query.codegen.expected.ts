/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { PrivateFragment, PublicFragment } from "../shared/fragments.codegen";
import { PrivateFragmentDocument, PublicFragmentDocument } from "../shared/fragments.codegen";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetMeQuery {
  __typename: "Query";
  me: ({ __typename: "User" } & PublicFragment) | null;
}

export type GetMeQueryVariables = Exact<{
}>;
export const GetMeQueryDocument = { kind: 'Document', definitions: [{"kind":"OperationDefinition","name":{"kind":"Name","value":"GetMe"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"me"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"FragmentSpread","name":{"kind":"Name","value":"PublicFragment"}}]}}]}}, PrivateFragmentDocument.definitions[0], PublicFragmentDocument.definitions[0]] } as unknown as DocumentNode<GetMeQuery, GetMeQueryVariables>;
