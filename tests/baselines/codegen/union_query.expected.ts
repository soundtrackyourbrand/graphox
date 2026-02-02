/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export type GetNodePolymorphicVariables = {
  id: string;
};

export type GetNodePolymorphic = {
  __typename: "Query";
  node: ({
    __typename: "Node";
  } & ({
      __typename: "User";
      id: string;
      username: string;
    } | {
      __typename: "Post";
      id: string;
      title: string;
    })) | null;
};
