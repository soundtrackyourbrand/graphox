/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export type PostPermissions = "READ" | "WRITE";

export type UserPermissions = "READ";

export interface Post {
  __typename: "Post";
  id: string;
  permissions: Array<PostPermissions>;
}

export interface Query {
  __typename: "Query";
  me: User;
}

export interface User {
  __typename: "User";
  id: string;
  permissions: UserPermissions;
  username: string;
}

