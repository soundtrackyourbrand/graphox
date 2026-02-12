/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { CreateUserMut, CreateUserMutVariables } from "./mutation.codegen";
import type { DeleteUserMut, DeleteUserMutVariables } from "./mutation.codegen";
import type { GetUserQ, GetUserQVariables } from "./query.codegen";
import type { GetUsersQ, GetUsersQVariables } from "./query.codegen";
import type { OnUserCreatedSub, OnUserCreatedSubVariables } from "./subscription.codegen";
import { CreateUserMutDocument } from "./mutation.codegen";
import { DeleteUserMutDocument } from "./mutation.codegen";
import { GetUserQDocument } from "./query.codegen";
import { GetUsersQDocument } from "./query.codegen";
import { OnUserCreatedSubDocument } from "./subscription.codegen";

const documents: { [key: string]: any } = {
  "mutation CreateUser($name: String!) {\n  createUser(name: $name) {\n    id\n    name\n  }\n}\n\nmutation DeleteUser($id: ID!) {\n  deleteUser(id: $id)\n}\n": CreateUserMutDocument,
  "query GetUsers {\n  users {\n    id\n    name\n  }\n}\n\nquery GetUser($id: ID!) {\n  user(id: $id) {\n    id\n    name\n  }\n}\n": GetUserQDocument,
  "subscription OnUserCreated {\n  userCreated {\n    id\n    name\n  }\n}\n": OnUserCreatedSubDocument,
};

export function graphql(source: "mutation CreateUser($name: String!) {\n  createUser(name: $name) {\n    id\n    name\n  }\n}\n\nmutation DeleteUser($id: ID!) {\n  deleteUser(id: $id)\n}\n"): typeof CreateUserMutDocument;
export function graphql(source: "query GetUsers {\n  users {\n    id\n    name\n  }\n}\n\nquery GetUser($id: ID!) {\n  user(id: $id) {\n    id\n    name\n  }\n}\n"): typeof GetUserQDocument;
export function graphql(source: "subscription OnUserCreated {\n  userCreated {\n    id\n    name\n  }\n}\n"): typeof OnUserCreatedSubDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
