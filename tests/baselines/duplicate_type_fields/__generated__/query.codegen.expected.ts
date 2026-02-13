/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";
import type { FragmentType } from "./fragment-masking";
import { DuplicateFields } from "./fragments.codegen";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetPlaylistQuery {
  __typename: "Query";
  playlist: Identity<({ __typename: "Playlist" } & { ' $fragmentRefs'?: { 'DuplicateFields': DuplicateFields } })> | null;
}

export type GetPlaylistQueryVariables = Exact<{
  id: string;
}>;

export const GetPlaylistQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"GetPlaylist"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"playlist"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"__typename"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"permissions"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"presentAs"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"snapshot"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"shortDescription"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"curated"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"curator"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"accountId"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null}]}},{"kind":"Field","name":{"kind":"Name","value":"presets"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"playbackMode"},"selectionSet":null}]}}]}}]},"variableDefinitions":[{"kind":"VariableDefinition","type":{"kind":"NonNullType","type":{"kind":"NamedType","name":{"kind":"Name","value":"ID"}}},"variable":{"kind":"Variable","name":{"kind":"Name","value":"id"}}}]},{"kind":"FragmentDefinition","name":{"kind":"Name","value":"DuplicateFields"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"__typename"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"permissions"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"presentAs"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"snapshot"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"shortDescription"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"curated"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"curator"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"accountId"},"selectionSet":null},{"kind":"Field","name":{"kind":"Name","value":"name"},"selectionSet":null}]}},{"kind":"Field","name":{"kind":"Name","value":"presets"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"playbackMode"},"selectionSet":null}]}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"Playlist"}}}],"kind":"Document"} as unknown as DocumentNode<GetPlaylistQuery, GetPlaylistQueryVariables>;

