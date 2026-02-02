/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export interface GetNodeQuery {
  __typename: "Query";
  node: { __typename: "Node" }
    | {
      __typename: "User";
      username: string;
    } | null;
}

export interface GetNodeQueryVariables {
  id: string;
}

