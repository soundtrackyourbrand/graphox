/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { FragmentType } from "./fragment-masking";
export type UserBasic = {
  __typename: "User";
  id: string;
  name: string;
} & { ' $fragmentName'?: 'UserBasic' };

export type UserExtended = {
  __typename: "User";
  name: string;
  email: string;
} & { ' $fragmentName'?: 'UserExtended' };

export type UserWithId = {
  __typename: "User";
  id: string;
} & { ' $fragmentName'?: 'UserWithId' };

export type UserFullName = {
  __typename: "User";
  name: string;
} & { ' $fragmentName'?: 'UserFullName' };

export type UserContact = {
  __typename: "User";
  email: string;
} & { ' $fragmentName'?: 'UserContact' };

export type UserNestedA = Identity<({ __typename: "User", role: string } & { ' $fragmentRefs'?: { 'UserFullName': UserFullName } })> & { ' $fragmentName'?: 'UserNestedA' };

export type UserNestedB = Identity<({ __typename: "User" } & { ' $fragmentRefs'?: { 'UserContact': UserContact, 'UserFullName': UserFullName } })> & { ' $fragmentName'?: 'UserNestedB' };

export type UserAliasedName = {
  __typename: "User";
  userName: string;
} & { ' $fragmentName'?: 'UserAliasedName' };

export type UserRealName = {
  __typename: "User";
  name: string;
} & { ' $fragmentName'?: 'UserRealName' };
export const UserAliasedNameDocument = { kind: 'Document', definitions: [{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserAliasedName"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":{"kind":"Name","value":"userName"},"kind":"Field","name":{"kind":"Name","value":"name"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}] } as unknown as DocumentNode<UserAliasedName, unknown> & {
  __fragment: UserAliasedName;
};
export const UserBasicDocument = { kind: 'Document', definitions: [{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserBasic"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"name"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}] } as unknown as DocumentNode<UserBasic, unknown> & {
  __fragment: UserBasic;
};
export const UserContactDocument = { kind: 'Document', definitions: [{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserContact"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"email"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}] } as unknown as DocumentNode<UserContact, unknown> & {
  __fragment: UserContact;
};
export const UserExtendedDocument = { kind: 'Document', definitions: [{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserExtended"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"name"}},{"kind":"Field","name":{"kind":"Name","value":"email"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}] } as unknown as DocumentNode<UserExtended, unknown> & {
  __fragment: UserExtended;
};
export const UserFullNameDocument = { kind: 'Document', definitions: [{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserFullName"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"name"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}] } as unknown as DocumentNode<UserFullName, unknown> & {
  __fragment: UserFullName;
};
export const UserNestedADocument = { kind: 'Document', definitions: [{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserNestedA"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"FragmentSpread","name":{"kind":"Name","value":"UserFullName"}},{"kind":"Field","name":{"kind":"Name","value":"role"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}] } as unknown as DocumentNode<UserNestedA, unknown> & {
  __fragment: UserNestedA;
};
export const UserNestedBDocument = { kind: 'Document', definitions: [{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserNestedB"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"FragmentSpread","name":{"kind":"Name","value":"UserFullName"}},{"kind":"FragmentSpread","name":{"kind":"Name","value":"UserContact"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}] } as unknown as DocumentNode<UserNestedB, unknown> & {
  __fragment: UserNestedB;
};
export const UserRealNameDocument = { kind: 'Document', definitions: [{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserRealName"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"name"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}] } as unknown as DocumentNode<UserRealName, unknown> & {
  __fragment: UserRealName;
};
export const UserWithIdDocument = { kind: 'Document', definitions: [{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserWithId"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}] } as unknown as DocumentNode<UserWithId, unknown> & {
  __fragment: UserWithId;
};
