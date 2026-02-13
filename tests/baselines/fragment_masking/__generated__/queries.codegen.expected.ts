/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { FragmentType } from "./fragment-masking";
import { UserEmail, UserFields } from "./fragments.codegen";
import { UserPosts } from "./post_fragment.codegen";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetUserQuery {
  __typename: "Query";
  user: Identity<({ __typename: "User" } & { ' $fragmentRefs'?: { 'UserFields': UserFields } })> | null;
}

export type GetUserQueryVariables = Exact<{
  id: string;
}>;

export const GetUserQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"GetUser"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"user"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}]},{"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserFields"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}],"kind":"Document"} as unknown as DocumentNode<GetUserQuery, GetUserQueryVariables>;

export interface GetUsersQuery {
  __typename: "Query";
  users: Array<Identity<({ __typename: "User" } & { ' $fragmentRefs'?: { 'UserEmail': UserEmail, 'UserFields': UserFields } })>> | null;
}

export type GetUsersQueryVariables = Exact<{
}>;

export const GetUsersQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"GetUsers"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"users"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"email"},"selectionSet":null}]}}]}},{"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserEmail"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"email"},"selectionSet":null}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}},{"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserFields"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}],"kind":"Document"} as unknown as DocumentNode<GetUsersQuery, GetUsersQueryVariables>;

export interface GetUserWithPostsQuery {
  __typename: "Query";
  user: Identity<({ __typename: "User", posts: Array<Identity<({ __typename: "Post" } & { ' $fragmentRefs'?: { 'UserPosts': UserPosts } })>> | null } & { ' $fragmentRefs'?: { 'UserFields': UserFields } })> | null;
}

export type GetUserWithPostsQueryVariables = Exact<{
  id: string;
}>;

export const GetUserWithPostsQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"GetUserWithPosts"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"user"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"posts"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"title"},"selectionSet":null}]}}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}]},{"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserFields"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}},{"kind":"FragmentDefinition","name":{"kind":"Name","value":"UserPosts"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"title"},"selectionSet":null}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"Post"}}}],"kind":"Document"} as unknown as DocumentNode<GetUserWithPostsQuery, GetUserWithPostsQueryVariables>;

