/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
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
export const DuplicateFieldsDocument = { kind: 'Document', definitions: [{"kind":"FragmentDefinition","name":{"kind":"Name","value":"DuplicateFields"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"__typename"}},{"kind":"Field","name":{"kind":"Name","value":"permissions"}},{"kind":"Field","name":{"kind":"Name","value":"presentAs"}},{"kind":"Field","name":{"kind":"Name","value":"name"}},{"kind":"Field","name":{"kind":"Name","value":"snapshot"}},{"kind":"Field","name":{"kind":"Name","value":"shortDescription"}},{"kind":"Field","name":{"kind":"Name","value":"curated"}},{"kind":"Field","name":{"kind":"Name","value":"curator"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"accountId"}},{"kind":"Field","name":{"kind":"Name","value":"name"}}]}},{"kind":"Field","name":{"kind":"Name","value":"presets"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"playbackMode"}}]}},{"kind":"Field","name":{"kind":"Name","value":"snapshot"}},{"kind":"Field","name":{"kind":"Name","value":"permissions"}},{"kind":"Field","name":{"kind":"Name","value":"presentAs"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"Playlist"}}}] } as unknown as DocumentNode<DuplicateFields, unknown> & {
  __fragment: DuplicateFields;
};
