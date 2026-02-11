/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { FragmentType } from "./fragment-masking";
export type UserBasic = {
  __typename: "User";
  id: string;
  name: string;
} & { ' $fragmentName'?: 'UserBasic' };

export declare const UserBasic: {
  __fragment: UserBasic;
};


export type UserExtended = {
  __typename: "User";
  name: string;
  email: string;
} & { ' $fragmentName'?: 'UserExtended' };

export declare const UserExtended: {
  __fragment: UserExtended;
};


export type UserWithId = {
  __typename: "User";
  id: string;
} & { ' $fragmentName'?: 'UserWithId' };

export declare const UserWithId: {
  __fragment: UserWithId;
};


export type UserFullName = {
  __typename: "User";
  name: string;
} & { ' $fragmentName'?: 'UserFullName' };

export declare const UserFullName: {
  __fragment: UserFullName;
};


export type UserContact = {
  __typename: "User";
  email: string;
} & { ' $fragmentName'?: 'UserContact' };

export declare const UserContact: {
  __fragment: UserContact;
};


export type UserNestedA = ({ __typename: "User", role: string } & { ' $fragmentRefs'?: { 'UserFullName': UserFullName } }) & { ' $fragmentName'?: 'UserNestedA' };

export declare const UserNestedA: {
  __fragment: UserNestedA;
};


export type UserNestedB = ({ __typename: "User" } & { ' $fragmentRefs'?: { 'UserContact': UserContact, 'UserFullName': UserFullName } }) & { ' $fragmentName'?: 'UserNestedB' };

export declare const UserNestedB: {
  __fragment: UserNestedB;
};


export type UserAliasedName = {
  __typename: "User";
  userName: string;
} & { ' $fragmentName'?: 'UserAliasedName' };

export declare const UserAliasedName: {
  __fragment: UserAliasedName;
};


export type UserRealName = {
  __typename: "User";
  name: string;
} & { ' $fragmentName'?: 'UserRealName' };

export declare const UserRealName: {
  __fragment: UserRealName;
};


