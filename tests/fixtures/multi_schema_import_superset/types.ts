/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export type Status = "ACTIVE" | "INACTIVE";

export interface Query {
  __typename: "Query";
  me?: User | null;
}

export interface User {
  __typename: "User";
  id: string;
  status: Status;
}

