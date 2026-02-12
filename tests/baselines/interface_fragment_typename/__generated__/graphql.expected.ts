/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { DisplayableInfo } from "./query.codegen";
import type { TestQueryQuery, TestQueryQueryVariables } from "./query.codegen";
import { TestQueryQueryDocument } from "./query.codegen";

const documents: { [key: string]: any } = {
  "\n  fragment DisplayableInfo on Displayable {\n    display {\n      title\n    }\n  }\n\n  query TestQuery {\n    items {\n      ...DisplayableInfo\n    }\n  }\n": TestQueryQueryDocument,
  "\n  fragment DisplayableInfo on Displayable {\n    display {\n      title\n    }\n  }\n\n  query TestQuery {\n    items {\n      ...DisplayableInfo\n    }\n  }\n": {},
};

export function graphql(source: "\n  fragment DisplayableInfo on Displayable {\n    display {\n      title\n    }\n  }\n\n  query TestQuery {\n    items {\n      ...DisplayableInfo\n    }\n  }\n"): typeof TestQueryQueryDocument;
export function graphql(source: "\n  fragment DisplayableInfo on Displayable {\n    display {\n      title\n    }\n  }\n\n  query TestQuery {\n    items {\n      ...DisplayableInfo\n    }\n  }\n"): DocumentNode<DisplayableInfo, unknown>;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
