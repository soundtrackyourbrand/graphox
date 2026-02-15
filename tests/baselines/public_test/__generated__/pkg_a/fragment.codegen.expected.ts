/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

export interface PublicFrag {
  __typename: "User";
  id: string;
}

export interface PrivateFrag {
  __typename: "User";
  id: string;
}
