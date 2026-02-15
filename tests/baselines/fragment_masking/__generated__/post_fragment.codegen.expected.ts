/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { FragmentType } from "./fragment-masking";
export type UserPosts = ({
  __typename: "Post";
  id: string;
  title: string | null;
}) & { ' $fragmentName'?: 'UserPosts' };
export const UserPostsDocument = { kind: 'Document', definitions: [{"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserPosts"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"title"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"Post"}}}] } as unknown as DocumentNode<UserPosts, unknown> & {
  __fragment: UserPosts;
};
