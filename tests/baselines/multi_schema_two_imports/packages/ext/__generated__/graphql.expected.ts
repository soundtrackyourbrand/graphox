/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;
import type { GetUserWithTaskQuery, GetUserWithTaskQueryVariables } from "./query.codegen";
import { GetUserWithTaskQueryDocument } from "./query.codegen";

const documents: { [key: string]: any } = {
  "# Query that uses types from BOTH schemas:\n# - UserStatus from base.graphql\n# - Priority from ext.graphql\nquery GetUserWithTask {\n  me {\n    id\n    name\n    status # Uses UserStatus from base schema\n  }\n  task(id: \"1\") {\n    id\n    title\n    priority # Uses Priority from ext schema\n  }\n}\n": GetUserWithTaskQueryDocument,
};

export function graphql(source: "# Query that uses types from BOTH schemas:\n# - UserStatus from base.graphql\n# - Priority from ext.graphql\nquery GetUserWithTask {\n  me {\n    id\n    name\n    status # Uses UserStatus from base schema\n  }\n  task(id: \"1\") {\n    id\n    title\n    priority # Uses Priority from ext schema\n  }\n}\n"): typeof GetUserWithTaskQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
