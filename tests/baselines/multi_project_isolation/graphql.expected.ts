/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { GetSettingsQuery, GetSettingsQueryVariables } from "./settings.codegen";
import type { GetUserQuery, GetUserQueryVariables } from "./user.codegen";
import { GetSettingsQueryDocument } from "./settings.codegen";
import { GetUserQueryDocument } from "./user.codegen";

const documents: { [key: string]: any } = {
  "query GetSettings {\n  settings {\n    theme\n    notifications\n  }\n}\n": GetSettingsQueryDocument,
  "query GetUser {\n  user {\n    id\n    name\n    email\n  }\n}\n": GetUserQueryDocument,
};

export function graphql(source: "query GetSettings {\n  settings {\n    theme\n    notifications\n  }\n}\n"): typeof GetSettingsQueryDocument;
export function graphql(source: "query GetUser {\n  user {\n    id\n    name\n    email\n  }\n}\n"): typeof GetUserQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
