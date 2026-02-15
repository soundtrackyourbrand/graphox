/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetUserByIdQuery {
  __typename: "Query";
  user: {
    __typename: "User";
    id: string;
    name: string | null;
    email: string | null;
  } | null;
}

export type GetUserByIdQueryVariables = Exact<{
  id: string;
}>;

export interface GetAllUsersQuery {
  __typename: "Query";
  users: Array<({ __typename: "User" } & UserFields) | null> | null;
}

export type GetAllUsersQueryVariables = Exact<{
}>;

export interface GetPostsWithFragmentQuery {
  __typename: "Query";
  posts: Array<({ __typename: "Post", author: {
      __typename: "User";
      id: string;
      name: string | null;
    } | null } & PostFields) | null> | null;
}

export type GetPostsWithFragmentQueryVariables = Exact<{
}>;

export interface CreatePostMutation {
  __typename: "Mutation";
  createPost: {
    __typename: "Post";
    id: string;
    title: string | null;
    content: string | null;
  } | null;
}

export type CreatePostMutationVariables = Exact<{
  title: string;
  content: string;
}>;

export interface GetUsersWithAddressesQuery {
  __typename: "Query";
  users: Array<({ __typename: "User" } & UserWithAddress) | null> | null;
}

export type GetUsersWithAddressesQueryVariables = Exact<{
}>;

export interface UserFields {
  __typename: "User";
  id: string;
  name: string | null;
  email: string | null;
}

export interface PostFields {
  __typename: "Post";
  id: string;
  title: string | null;
  content: string | null;
}

export interface AddressFields {
  __typename: "Address";
  street: string | null;
  city: string | null;
  country: string | null;
}

export type UserWithAddress = ({ __typename: "User", id: string, name: string | null } & AddressFields);
export const CreatePostMutationDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"create_post"},"operation":"mutation","selectionSet":{"kind":"SelectionSet","selections":[{"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"title"},"value":{"kind":"Variable","name":{"kind":"Name","value":"title"}}},{"kind":"Argument","name":{"kind":"Name","value":"content"},"value":{"kind":"Variable","name":{"kind":"Name","value":"content"}}}],"kind":"Field","name":{"kind":"Name","value":"createPost"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"title"}},{"kind":"Field","name":{"kind":"Name","value":"content"}}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"String"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"title"}}},{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"String"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"content"}}}]}],"kind":"Document"} as unknown as DocumentNode<CreatePostMutation, CreatePostMutationVariables>;
export const GetAllUsersQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"get_all_users"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"users"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"name"}},{"kind":"Field","name":{"kind":"Name","value":"email"}}]}}]}},{"kind":"FragmentDefinition","name":{"kind":"Name","value":"user_fields"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"name"}},{"kind":"Field","name":{"kind":"Name","value":"email"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}],"kind":"Document"} as unknown as DocumentNode<GetAllUsersQuery, GetAllUsersQueryVariables>;
export const GetPostsWithFragmentQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"get_posts_with_fragment"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"posts"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"title"}},{"kind":"Field","name":{"kind":"Name","value":"content"}},{"kind":"Field","name":{"kind":"Name","value":"author"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"name"}}]}}]}}]}},{"kind":"FragmentDefinition","name":{"kind":"Name","value":"post_fields"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"title"}},{"kind":"Field","name":{"kind":"Name","value":"content"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"Post"}}}],"kind":"Document"} as unknown as DocumentNode<GetPostsWithFragmentQuery, GetPostsWithFragmentQueryVariables>;
export const GetUserByIdQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"get_user_by_id"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"arguments":[{"kind":"Argument","name":{"kind":"Name","value":"id"},"value":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}],"kind":"Field","name":{"kind":"Name","value":"user"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"name"}},{"kind":"Field","name":{"kind":"Name","value":"email"}}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}]}],"kind":"Document"} as unknown as DocumentNode<GetUserByIdQuery, GetUserByIdQueryVariables>;
export const GetUsersWithAddressesQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"get_users_with_addresses"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"users"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"name"}},{"kind":"Field","name":{"kind":"Name","value":"street"}},{"kind":"Field","name":{"kind":"Name","value":"city"}},{"kind":"Field","name":{"kind":"Name","value":"country"}}]}}]}},{"kind":"FragmentDefinition","name":{"kind":"Name","value":"address_fields"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"street"}},{"kind":"Field","name":{"kind":"Name","value":"city"}},{"kind":"Field","name":{"kind":"Name","value":"country"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"Address"}}},{"kind":"FragmentDefinition","name":{"kind":"Name","value":"user_with_address"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"name"}},{"kind":"Field","name":{"kind":"Name","value":"street"}},{"kind":"Field","name":{"kind":"Name","value":"city"}},{"kind":"Field","name":{"kind":"Name","value":"country"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"User"}}}],"kind":"Document"} as unknown as DocumentNode<GetUsersWithAddressesQuery, GetUsersWithAddressesQueryVariables>;
