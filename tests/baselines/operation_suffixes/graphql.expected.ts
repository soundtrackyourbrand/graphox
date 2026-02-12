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
};

export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;
