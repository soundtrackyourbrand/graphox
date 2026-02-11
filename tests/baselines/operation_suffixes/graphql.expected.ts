/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import { CreateUserMut, CreateUserMutVariables, CreateUserMutDocument } from "./mutation.codegen";
import { DeleteUserMut, DeleteUserMutVariables, DeleteUserMutDocument } from "./mutation.codegen";
import { GetUserQ, GetUserQVariables, GetUserQDocument } from "./query.codegen";
import { GetUsersQ, GetUsersQVariables, GetUsersQDocument } from "./query.codegen";
import { OnUserCreatedSub, OnUserCreatedSubVariables, OnUserCreatedSubDocument } from "./subscription.codegen";

const documents: { [key: string]: any } = {
  "mutation CreateUser($name: String!) {\n  createUser(name: $name) {\n    id\n    name\n  }\n}\n\nmutation DeleteUser($id: ID!) {\n  deleteUser(id: $id)\n}\n": CreateUserMutDocument,
  "mutation CreateUser($name: String!) {\n  createUser(name: $name) {\n    id\n    name\n  }\n}\n\nmutation DeleteUser($id: ID!) {\n  deleteUser(id: $id)\n}\n": DeleteUserMutDocument,
  "query GetUsers {\n  users {\n    id\n    name\n  }\n}\n\nquery GetUser($id: ID!) {\n  user(id: $id) {\n    id\n    name\n  }\n}\n": GetUsersQDocument,
  "query GetUsers {\n  users {\n    id\n    name\n  }\n}\n\nquery GetUser($id: ID!) {\n  user(id: $id) {\n    id\n    name\n  }\n}\n": GetUserQDocument,
  "subscription OnUserCreated {\n  userCreated {\n    id\n    name\n  }\n}\n": OnUserCreatedSubDocument,
};

export function graphql(source: "mutation CreateUser($name: String!) {\n  createUser(name: $name) {\n    id\n    name\n  }\n}\n\nmutation DeleteUser($id: ID!) {\n  deleteUser(id: $id)\n}\n"): typeof CreateUserMutDocument;
export function graphql(source: "mutation CreateUser($name: String!) {\n  createUser(name: $name) {\n    id\n    name\n  }\n}\n\nmutation DeleteUser($id: ID!) {\n  deleteUser(id: $id)\n}\n"): typeof DeleteUserMutDocument;
export function graphql(source: "query GetUsers {\n  users {\n    id\n    name\n  }\n}\n\nquery GetUser($id: ID!) {\n  user(id: $id) {\n    id\n    name\n  }\n}\n"): typeof GetUsersQDocument;
export function graphql(source: "query GetUsers {\n  users {\n    id\n    name\n  }\n}\n\nquery GetUser($id: ID!) {\n  user(id: $id) {\n    id\n    name\n  }\n}\n"): typeof GetUserQDocument;
export function graphql(source: "subscription OnUserCreated {\n  userCreated {\n    id\n    name\n  }\n}\n"): typeof OnUserCreatedSubDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
