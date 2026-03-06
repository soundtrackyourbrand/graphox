/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
export interface PublicFragment {
  __typename: "User";
  id: string;
  name: string;
  profile: ({ __typename: "Profile" } & PrivateFragment) | null;
}

export interface PrivateFragment {
  __typename: "Profile";
  bio: string | null;
}
export const PrivateFragmentDocument = { kind: 'Document', definitions: [{"kind":"FragmentDefinition","name":{"kind":"Name","value":"PrivateFragment"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"bio"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"Profile"}}}] } as unknown as DocumentNode<PrivateFragment, unknown>;
export const PublicFragmentDocument = { kind: 'Document', definitions: [{"kind":"FragmentDefinition","name":{"kind":"Name","value":"PublicFragment"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"name"}},{"kind":"Field","name":{"kind":"Name","value":"profile"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"FragmentSpread","name":{"kind":"Name","value":"PrivateFragment"}}]}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}, PrivateFragmentDocument.definitions[0]] } as unknown as DocumentNode<PublicFragment, unknown>;
