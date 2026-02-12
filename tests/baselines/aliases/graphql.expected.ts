/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { GetComplexUserQuery, GetComplexUserQueryVariables } from "./query.codegen";
import { GetComplexUserQueryDocument } from "./query.codegen";

const documents: { [key: string]: any } = {
  "query GetComplexUser($id: ID!) {\n    currentUser: me {\n        uid: id\n        name: username\n    }\n    otherUser: user(id: $id) {\n        id\n        alias1: username\n        alias2: username\n        connections: friends {\n            friendId: id\n            friendName: username\n        }\n    }\n}\n": GetComplexUserQueryDocument,
};

export function graphql(source: "query GetComplexUser($id: ID!) {\n    currentUser: me {\n        uid: id\n        name: username\n    }\n    otherUser: user(id: $id) {\n        id\n        alias1: username\n        alias2: username\n        connections: friends {\n            friendId: id\n            friendName: username\n        }\n    }\n}\n"): typeof GetComplexUserQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
