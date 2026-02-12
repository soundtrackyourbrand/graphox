/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { FragmentType } from "./fragment-masking";
export type UserPosts = ({
  __typename: "Post";
  id: string;
  title: string | null;
}) & { ' $fragmentName'?: 'UserPosts' };

export declare const UserPostsDocument: {
  __fragment: UserPosts;
};


