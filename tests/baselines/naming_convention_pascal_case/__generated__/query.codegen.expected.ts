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

export const GetUserByIdQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"get_user_by_id"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"user"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"email"},"selectionSet":null}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}]}],"kind":"Document"} as unknown as DocumentNode<GetUserByIdQuery, GetUserByIdQueryVariables>;

export interface GetAllPostsQuery {
  __typename: "Query";
  posts: Array<{
    __typename: "Post";
    id: string;
    title: string | null;
    content: string | null;
    author: {
      __typename: "User";
      id: string;
      name: string | null;
    } | null;
  } | null> | null;
}

export type GetAllPostsQueryVariables = Exact<{
}>;

export const GetAllPostsQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"get_all_posts"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"posts"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"title"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"content"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"author"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null}]}}]}}]}}],"kind":"Document"} as unknown as DocumentNode<GetAllPostsQuery, GetAllPostsQueryVariables>;

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

export const CreatePostMutationDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"create_post"},"operation":"mutation","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"createPost"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"title"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"content"},"selectionSet":null}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"String"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"title"}}},{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"String"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"content"}}}]}],"kind":"Document"} as unknown as DocumentNode<CreatePostMutation, CreatePostMutationVariables>;

export interface GenerateOtpQuery {
  __typename: "Query";
  generateOTP: {
    __typename: "GenerateOTPPayload";
    otp: string | null;
    expiresAt: string | null;
  } | null;
}

export type GenerateOtpQueryVariables = Exact<{
  input: any;
}>;

export const GenerateOtpQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"generateOTP"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"generateOTP"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"otp"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"expiresAt"},"selectionSet":null}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"GenerateOTPInput"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"input"}}}]}],"kind":"Document"} as unknown as DocumentNode<GenerateOtpQuery, GenerateOtpQueryVariables>;

export interface GetSamlConfigQuery {
  __typename: "Query";
  samlConfig: {
    __typename: "SAMLConfig";
    expiresAt: string | null;
    slug: string | null;
  } | null;
}

export type GetSamlConfigQueryVariables = Exact<{
  accountId: string;
}>;

export const GetSamlConfigQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"getSAMLConfig"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"samlConfig"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"expiresAt"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"slug"},"selectionSet":null}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"accountId"}}}]}],"kind":"Document"} as unknown as DocumentNode<GetSamlConfigQuery, GetSamlConfigQueryVariables>;

export interface AddTracks_CreateManualPlaylistMutation {
  __typename: "Mutation";
  createManualPlaylist: {
    __typename: "Playlist";
    id: string;
    permissions: Array<string | null> | null;
  } | null;
}

export type AddTracks_CreateManualPlaylistMutationVariables = Exact<{
  input: any;
}>;

export const AddTracks_CreateManualPlaylistMutationDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"AddTracks_CreateManualPlaylist"},"operation":"mutation","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"createManualPlaylist"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"permissions"},"selectionSet":null}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"CreateManualPlaylistInput"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"input"}}}]}],"kind":"Document"} as unknown as DocumentNode<AddTracks_CreateManualPlaylistMutation, AddTracks_CreateManualPlaylistMutationVariables>;

export interface ChangePlan_AccountQuery {
  __typename: "Query";
  account: {
    __typename: "User";
    id: string;
    name: string | null;
  } | null;
}

export type ChangePlan_AccountQueryVariables = Exact<{
  accountId: string;
}>;

export const ChangePlan_AccountQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"ChangePlan_account"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"account"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"accountId"}}}]}],"kind":"Document"} as unknown as DocumentNode<ChangePlan_AccountQuery, ChangePlan_AccountQueryVariables>;

export interface ChangePlan_PricesQuery {
  __typename: "Query";
  prices: Array<{
    __typename: "Price";
    id: string;
    amount: number | null;
  } | null> | null;
}

export type ChangePlan_PricesQueryVariables = Exact<{
}>;

export const ChangePlan_PricesQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"ChangePlan_prices"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"prices"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"amount"},"selectionSet":null}]}}]}}],"kind":"Document"} as unknown as DocumentNode<ChangePlan_PricesQuery, ChangePlan_PricesQueryVariables>;

export interface UserFields {
  __typename: "User";
  id: string;
  name: string | null;
  email: string | null;
}

