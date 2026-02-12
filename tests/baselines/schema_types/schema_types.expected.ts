/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

/**
 * User roles in the system
 */
export type Role = "ADMIN" | "USER";

/**
 * Input for creating a user
 */
export interface CreateUserInput {
  /**
   * The username of the new user
   */
  username: string;
  role?: Role | null;
  /**
   * The old way to set role
   *
   * @deprecated Use role instead
   */
  oldRole?: string | null;
}

/**
 * Deprecated input type
 */
export interface OldInput {
  id: string;
}

export interface Query {
  __typename: "Query";
  user?: string | null;
}
