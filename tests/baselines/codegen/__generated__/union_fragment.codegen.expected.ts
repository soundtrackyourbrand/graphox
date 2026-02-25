/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

export interface AccountInfo {
    __typename: "User";
    username: string;
  }
  | {
    __typename: "Admin";
    role: string;
  }
