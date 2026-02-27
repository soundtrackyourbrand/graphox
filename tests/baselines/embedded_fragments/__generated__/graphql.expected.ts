/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;
import { Auth_AccountWithSettingsDocument } from "./queries.codegen";
import { Auth_FullAccountSettingsDocument } from "./queries.codegen";

const documents: { [key: string]: any } = {
  "\n  fragment Auth_AccountWithSettings on Account {\n    id\n    permissions\n    settings {\n      ...Auth_FullAccountSettings\n    }\n  }\n\n  fragment Auth_FullAccountSettings on AccountSettings {\n    filterExplicit\n    restrictBlockTracks\n    restrictDiscoverMusic\n    restrictEditMusic\n    restrictUnpairingFromPairedDevices\n  }\n": Auth_AccountWithSettingsDocument,
};

export function graphql(source: "\n  fragment Auth_AccountWithSettings on Account {\n    id\n    permissions\n    settings {\n      ...Auth_FullAccountSettings\n    }\n  }\n\n  fragment Auth_FullAccountSettings on AccountSettings {\n    filterExplicit\n    restrictBlockTracks\n    restrictDiscoverMusic\n    restrictEditMusic\n    restrictUnpairingFromPairedDevices\n  }\n"): typeof Auth_AccountWithSettingsDocument;
export function graphql<Result, Variables>(source: string): DocumentNode<Result, Variables>;
export function graphql(source: string): any {
  return documents[source] || {};
}

export const gql = graphql;

// Re-exports
export type { Auth_AccountWithSettings, Auth_FullAccountSettings } from "./queries.codegen";
export { Auth_AccountWithSettingsDocument, Auth_FullAccountSettingsDocument } from "./queries.codegen";
