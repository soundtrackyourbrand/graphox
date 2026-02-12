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
  "# Scenario 1: Overlapping fragments - both have 'name' field\n# This should NOT produce duplicate 'name' field in the AST\nquery GetUserWithOverlappingFragments($id: ID!) {\n  user(id: $id) {\n    ...UserBasic\n    ...UserExtended\n  }\n}\n": GetUserWithOverlappingFragmentsQueryDocument,
  "# Scenario 2: Inline field + fragment with same field\n# Inline 'name' + fragment with 'id' - should not duplicate\nquery GetUserWithInlineAndFragment($id: ID!) {\n  user(id: $id) {\n    name\n    ...UserWithId\n  }\n}\n": GetUserWithInlineAndFragmentQueryDocument,
  "# Scenario 3: Nested fragments with overlapping fields\n# UserNestedA and UserNestedB both include UserFullName (which has 'name')\n# This should NOT produce duplicate 'name' field in the AST\nquery GetUsersWithNestedFragments {\n  users {\n    ...UserNestedA\n    ...UserNestedB\n  }\n}\n": GetUsersWithNestedFragmentsQueryDocument,
  "# Scenario 4: Aliases - different keys, should BOTH appear\n# 'userName' and 'name' are different field names, so both should appear\nquery GetUserWithAliasedFragments($id: ID!) {\n  user(id: $id) {\n    ...UserAliasedName\n    ...UserRealName\n  }\n}\n": GetUserWithAliasedFragmentsQueryDocument,
};

export function graphql(source: "# Scenario 1: Overlapping fragments - both have 'name' field\n# This should NOT produce duplicate 'name' field in the AST\nquery GetUserWithOverlappingFragments($id: ID!) {\n  user(id: $id) {\n    ...UserBasic\n    ...UserExtended\n  }\n}\n"): typeof GetUserWithOverlappingFragmentsQueryDocument;
export function graphql(source: "# Scenario 2: Inline field + fragment with same field\n# Inline 'name' + fragment with 'id' - should not duplicate\nquery GetUserWithInlineAndFragment($id: ID!) {\n  user(id: $id) {\n    name\n    ...UserWithId\n  }\n}\n"): typeof GetUserWithInlineAndFragmentQueryDocument;
export function graphql(source: "# Scenario 3: Nested fragments with overlapping fields\n# UserNestedA and UserNestedB both include UserFullName (which has 'name')\n# This should NOT produce duplicate 'name' field in the AST\nquery GetUsersWithNestedFragments {\n  users {\n    ...UserNestedA\n    ...UserNestedB\n  }\n}\n"): typeof GetUsersWithNestedFragmentsQueryDocument;
export function graphql(source: "# Scenario 4: Aliases - different keys, should BOTH appear\n# 'userName' and 'name' are different field names, so both should appear\nquery GetUserWithAliasedFragments($id: ID!) {\n  user(id: $id) {\n    ...UserAliasedName\n    ...UserRealName\n  }\n}\n"): typeof GetUserWithAliasedFragmentsQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
