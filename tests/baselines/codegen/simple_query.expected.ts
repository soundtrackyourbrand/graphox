/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export type GetUsersVariables = Record<string, never>;

export type GetUsers = {
  __typename: "Query";
  users: Array<{
    __typename: "User";
    id: string;
    username: string;
  } | null> | null;
};
