/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { GetUserQuery, GetUserQueryVariables } from "./queries/user/get_user.codegen";
import type { GetUserQuery, GetUserQueryVariables } from "./test.codegen";
import { GetUserQueryDocument } from "./queries/user/get_user.codegen";
import { GetUserQueryDocument } from "./test.codegen";

const documents: { [key: string]: any } = {
};

export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
