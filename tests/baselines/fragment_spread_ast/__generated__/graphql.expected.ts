/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;
import type { AppQueryQuery, AppQueryQueryVariables } from "../fragments.codegen";
import { AppQueryQueryDocument } from "../fragments.codegen";
import { FragADocument } from "../fragments.codegen";
import { FragBDocument } from "../fragments.codegen";
import { FragCDocument } from "../fragments.codegen";

const documents: { [key: string]: any } = {
  "fragment FragC on User {\n  id\n}\n\nfragment FragB on User {\n  name\n  ...FragC\n}\n\nfragment FragA on User {\n  email\n  ...FragB\n  ...FragC\n}\n\nquery AppQuery($id: ID!) {\n  user(id: $id) {\n    ...FragA\n  }\n}\n": AppQueryQueryDocument,
};

export function graphql(source: "fragment FragC on User {\n  id\n}\n\nfragment FragB on User {\n  name\n  ...FragC\n}\n\nfragment FragA on User {\n  email\n  ...FragB\n  ...FragC\n}\n\nquery AppQuery($id: ID!) {\n  user(id: $id) {\n    ...FragA\n  }\n}\n"): typeof AppQueryQueryDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
