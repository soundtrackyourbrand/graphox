/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
export interface UserFields {
  __typename: "User";
  id: string;
  name: string | null;
}
export const UserFieldsDocument = { kind: 'Document', definitions: [{"directives":[],"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserFields"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}] } as unknown as DocumentNode<UserFields, unknown>;

