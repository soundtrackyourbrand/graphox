/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { PostPermissions, UserPermissions } from "./schema.types";

export interface PermissionTypes {
  Post: PostPermissions | null;
  User: UserPermissions;
}

export const permissionTypes = {
  Post: ['READ', 'WRITE'],
  User: ['READ'],
}
