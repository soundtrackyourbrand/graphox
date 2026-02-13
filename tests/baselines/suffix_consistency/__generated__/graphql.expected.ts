/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;
import type { GetUserQuery, GetUserQueryVariables } from "./query.codegen";
import type { UpdateUser, UpdateUserVariables } from "./mutation.codegen";
import { GetUserQueryDoc } from "./query.codegen";
import { UpdateUserDoc } from "./mutation.codegen";
import { UserFieldsFragFragmentDoc } from "./fragment.codegen";

const documents: { [key: string]: any } = {
  "mutation UpdateUser($name: String!) { updateUser(name: $name) { id name } }\n": UpdateUserDoc,
  "query GetUser { user { ...UserFields } }\n": GetUserQueryDoc,
  "fragment UserFields on User { id name }\n": UserFieldsFragFragmentDoc,
};

export function graphql(source: "mutation UpdateUser($name: String!) { updateUser(name: $name) { id name } }\n"): typeof UpdateUserDoc;
export function graphql(source: "query GetUser { user { ...UserFields } }\n"): typeof GetUserQueryDoc;
export function graphql(source: "fragment UserFields on User { id name }\n"): typeof UserFieldsFragFragmentDoc;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
