/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;
import type { GetSettingsQuery, GetSettingsQueryVariables } from "./settings.codegen";
import { GetSettingsQueryDocument } from "./settings.codegen";

const documents: { [key: string]: any } = {
  "query GetSettings {\n  settings {\n    theme\n    notifications\n  }\n}\n": GetSettingsQueryDocument,
};

export function graphql(source: "query GetSettings {\n  settings {\n    theme\n    notifications\n  }\n}\n"): typeof GetSettingsQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
