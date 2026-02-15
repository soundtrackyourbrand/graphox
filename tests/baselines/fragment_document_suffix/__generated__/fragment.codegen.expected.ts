/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { FragmentType } from "./fragment-masking";
export type UserFieldsFrag = ({
  __typename: "User";
  id: string;
  name: string | null;
}) & { ' $fragmentName'?: 'UserFieldsFrag' };
export const UserFieldsFragDoc = { kind: 'Document', definitions: [{"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserFields"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"name"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}] } as unknown as DocumentNode<UserFieldsFrag, unknown> & {
  __fragment: UserFieldsFrag;
};
