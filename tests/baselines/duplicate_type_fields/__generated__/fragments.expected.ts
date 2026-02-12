/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

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

export declare const DuplicateFields: {
  __fragment: DuplicateFields;
};


