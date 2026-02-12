/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

export interface UserFields {
  __typename: "User";
  id: string;
  username: string;
}

