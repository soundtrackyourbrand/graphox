/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { FragmentType } from "./fragment-masking";
export type UserFields = ({
  __typename: "User";
  id: string;
  name: string | null;
}) & { ' $fragmentName'?: 'UserFields' };

export type UserEmail = ({
  __typename: "User";
  email: string | null;
}) & { ' $fragmentName'?: 'UserEmail' };
export const UserEmailDocument = { kind: 'Document', definitions: [{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserEmail"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"email"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}] } as unknown as DocumentNode<UserEmail, unknown> & {
  __fragment: UserEmail;
};
export const UserFieldsDocument = { kind: 'Document', definitions: [{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserFields"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"name"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}] } as unknown as DocumentNode<UserFields, unknown> & {
  __fragment: UserFields;
};
