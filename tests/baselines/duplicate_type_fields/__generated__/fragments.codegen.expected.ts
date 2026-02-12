/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { FragmentType } from "./fragment-masking";
export type DuplicateFields = ({
  id: string;
  __typename: "Playlist";
  permissions: Array<"ADMIN" | "READ" | "WRITE">;
  presentAs: "CAROUSEL" | "GRID" | "LIST";
  name: string;
  snapshot: string;
  shortDescription: string | null;
  curated: boolean;
  curator: {
    __typename: "Curator";
    id: string;
    accountId: string;
    name: string;
  } | null;
  presets: Array<{
    __typename: "Preset";
    playbackMode: "LOOP" | "SEQUENTIAL" | "SHUFFLE";
  }>;
}) & { ' $fragmentName'?: 'DuplicateFields' };

export declare const DuplicateFieldsDocument: {
  __fragment: DuplicateFields;
};


