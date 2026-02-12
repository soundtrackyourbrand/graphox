/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { FragmentType } from "./fragment-masking";
export type UserBasic = {
  __typename: "User";
  id: string;
  name: string;
} & { ' $fragmentName'?: 'UserBasic' };

export declare const UserBasicDocument: {
  __fragment: UserBasic;
};


export type UserExtended = {
  __typename: "User";
  name: string;
  email: string;
} & { ' $fragmentName'?: 'UserExtended' };

export declare const UserExtendedDocument: {
  __fragment: UserExtended;
};


export type UserWithId = {
  __typename: "User";
  id: string;
} & { ' $fragmentName'?: 'UserWithId' };

export declare const UserWithIdDocument: {
  __fragment: UserWithId;
};


export type UserFullName = {
  __typename: "User";
  name: string;
} & { ' $fragmentName'?: 'UserFullName' };

export declare const UserFullNameDocument: {
  __fragment: UserFullName;
};


export type UserContact = {
  __typename: "User";
  email: string;
} & { ' $fragmentName'?: 'UserContact' };

export declare const UserContactDocument: {
  __fragment: UserContact;
};


export type UserNestedA = Identity<({ __typename: "User", role: string } & { ' $fragmentRefs'?: { 'UserFullName': UserFullName } })> & { ' $fragmentName'?: 'UserNestedA' };

export declare const UserNestedADocument: {
  __fragment: UserNestedA;
};


export type UserNestedB = Identity<({ __typename: "User" } & { ' $fragmentRefs'?: { 'UserContact': UserContact, 'UserFullName': UserFullName } })> & { ' $fragmentName'?: 'UserNestedB' };

export declare const UserNestedBDocument: {
  __fragment: UserNestedB;
};


export type UserAliasedName = {
  __typename: "User";
  userName: string;
} & { ' $fragmentName'?: 'UserAliasedName' };

export declare const UserAliasedNameDocument: {
  __fragment: UserAliasedName;
};


export type UserRealName = {
  __typename: "User";
  name: string;
} & { ' $fragmentName'?: 'UserRealName' };

export declare const UserRealNameDocument: {
  __fragment: UserRealName;
};


