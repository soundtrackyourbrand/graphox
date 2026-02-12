/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export interface Post {
  __typename: "Post";
  id: string;
  title: string;
}

export interface Query {
  __typename: "Query";
  me?: User | null;
  search: Array<SearchResult>;
}

export interface User {
  __typename: "User";
  id: string;
  username: string;
}

export interface Node {
  __typename: "Post" | "User";
  id: string;
}

export type SearchResult = Post | User;
