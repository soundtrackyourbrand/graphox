/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { UserFieldsFragment } from "./fragment_definition.codegen";

export type GetUsersWithFragmentVariables = Record<string, never>;

export type GetUsersWithFragment = {
  __typename: "Query";
  users: Array<({
    __typename: "User";
    email: string | null;
  } & UserFieldsFragment) | null> | null;
};
