/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { UserFields } from "./fragment.codegen";
import { UserFieldsDocument } from "./fragment.codegen";

export interface GetMeQuery {
  __typename: "Query";
  me: ({ __typename: "User" } & UserFields) | null;
}

export const GetMeQueryDocument = { kind: 'Document', definitions: [{"directives":[],"kind":"OperationDefinition","name":{"kind":"Name","value":"GetMe"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"me"},"selectionSet":{"kind":"SelectionSet","selections":[{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"alias":null,"arguments":[],"directives":[],"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null}]}}]},"variableDefinitions":[]}, ...UserFieldsDocument.definitions] } as unknown as DocumentNode<GetMeQuery, { [key: string]: never; }>;

