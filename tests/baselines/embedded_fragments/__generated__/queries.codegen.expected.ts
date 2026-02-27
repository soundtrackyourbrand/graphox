/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
export interface Auth_AccountWithSettings {
  __typename: "Account";
  id: string;
  permissions: Array<string>;
  settings: ({ __typename: "AccountSettings" } & Auth_FullAccountSettings) | null;
}

export interface Auth_FullAccountSettings {
  __typename: "AccountSettings";
  filterExplicit: boolean;
  restrictBlockTracks: boolean;
  restrictDiscoverMusic: boolean;
  restrictEditMusic: boolean;
  restrictUnpairingFromPairedDevices: boolean;
}
export const Auth_FullAccountSettingsDocument = { kind: 'Document', definitions: [{"kind":"FragmentDefinition","name":{"kind":"Name","value":"Auth_FullAccountSettings"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"filterExplicit"}},{"kind":"Field","name":{"kind":"Name","value":"restrictBlockTracks"}},{"kind":"Field","name":{"kind":"Name","value":"restrictDiscoverMusic"}},{"kind":"Field","name":{"kind":"Name","value":"restrictEditMusic"}},{"kind":"Field","name":{"kind":"Name","value":"restrictUnpairingFromPairedDevices"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"AccountSettings"}}}] } as unknown as DocumentNode<Auth_FullAccountSettings, unknown>;
export const Auth_AccountWithSettingsDocument = { kind: 'Document', definitions: [{"kind":"FragmentDefinition","name":{"kind":"Name","value":"Auth_AccountWithSettings"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"permissions"}},{"kind":"Field","name":{"kind":"Name","value":"settings"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"FragmentSpread","name":{"kind":"Name","value":"Auth_FullAccountSettings"}}]}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"Account"}}}, Auth_FullAccountSettingsDocument.definitions[0]] } as unknown as DocumentNode<Auth_AccountWithSettings, unknown>;
