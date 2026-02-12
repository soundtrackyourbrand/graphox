/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export type Priority = "CRITICAL" | "HIGH" | "LOW" | "MEDIUM";

export type UserStatus = "ACTIVE" | "INACTIVE" | "PENDING";

export interface Query {
  __typename: "Query";
  me?: User | null;
  task?: Task | null;
}

export interface Task {
  __typename: "Task";
  id: string;
  priority: Priority;
  title: string;
}

export interface User {
  __typename: "User";
  id: string;
  name: string;
  status: UserStatus;
}

