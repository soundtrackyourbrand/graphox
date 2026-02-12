/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { FragmentType } from "./fragment-masking";
import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { GetUserWithAliasedFragmentsQuery, GetUserWithAliasedFragmentsQueryVariables } from "./scenario4_aliases.codegen";
import type { GetUserWithInlineAndFragmentQuery, GetUserWithInlineAndFragmentQueryVariables } from "./scenario2_inline.codegen";
import type { GetUserWithOverlappingFragmentsQuery, GetUserWithOverlappingFragmentsQueryVariables } from "./scenario1_overlapping.codegen";
import type { GetUsersWithNestedFragmentsQuery, GetUsersWithNestedFragmentsQueryVariables } from "./scenario3_nested.codegen";
import { GetUserWithAliasedFragmentsQueryDocument } from "./scenario4_aliases.codegen";
import { GetUserWithInlineAndFragmentQueryDocument } from "./scenario2_inline.codegen";
import { GetUserWithOverlappingFragmentsQueryDocument } from "./scenario1_overlapping.codegen";
import { GetUsersWithNestedFragmentsQueryDocument } from "./scenario3_nested.codegen";

const documents: { [key: string]: any } = {
};

export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
