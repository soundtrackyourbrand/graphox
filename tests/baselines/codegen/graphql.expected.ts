/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { GetNodePolymorphicQuery, GetNodePolymorphicQueryVariables } from "./union_query.codegen";
import type { GetNodeQuery, GetNodeQueryVariables } from "./query_with_variables.codegen";
import type { GetUsersQuery, GetUsersQueryVariables } from "./simple_query.codegen";
import type { GetUsersWithFragmentQuery, GetUsersWithFragmentQueryVariables } from "./fragment_usage.codegen";
import { GetNodePolymorphicQueryDocument } from "./union_query.codegen";
import { GetNodeQueryDocument } from "./query_with_variables.codegen";
import { GetUsersQueryDocument } from "./simple_query.codegen";
import { GetUsersWithFragmentQueryDocument } from "./fragment_usage.codegen";

const documents: { [key: string]: any } = {
};

export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
