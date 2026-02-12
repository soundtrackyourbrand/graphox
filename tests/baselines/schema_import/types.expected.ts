/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export type Role = "ADMIN" | "USER";

export interface Query {
  __typename: "Query";
  me?: User | null;
}

export interface User {
  __typename: "User";
  id: string;
  role: Role;
}

