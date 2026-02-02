/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export type GetNodeVariables = {
  id: string;
};

export type GetNode = {
  __typename: "Query";
  node: ({
    __typename: "Comment" | "Post" | "User";
    id: string;
  } & ({
      __typename: "User";
      username: string;
    })) | null;
};
