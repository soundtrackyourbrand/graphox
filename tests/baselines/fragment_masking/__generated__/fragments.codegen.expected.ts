/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { FragmentType } from "./fragment-masking";
export type UserFields = ({
  __typename: "User";
  id: string;
  name: string | null;
}) & { ' $fragmentName'?: 'UserFields' };

export declare const UserFieldsDocument: {
  __fragment: UserFields;
};


export type UserEmail = ({
  __typename: "User";
  email: string | null;
}) & { ' $fragmentName'?: 'UserEmail' };

export declare const UserEmailDocument: {
  __fragment: UserEmail;
};


