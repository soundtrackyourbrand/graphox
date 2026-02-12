/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { FragmentType } from "./fragment-masking";
export type UserFieldsFrag = ({
  __typename: "User";
  id: string;
  name: string | null;
}) & { ' $fragmentName'?: 'UserFieldsFrag' };

export declare const UserFieldsDoc: {
  __fragment: UserFieldsFrag;
};


