/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface GetPlaybackQuery {
  __typename: "Query";
  playback: {
    __typename: "Playback";
    id: string;
    playable: {
        __typename: "Track";
        id: string;
        title: string;
        durationMs: number | null;
      }
      | {
        __typename: "Artist";
        id: string;
        title: string;
      } | null;
  } | null;
}

export type GetPlaybackQueryVariables = Exact<{
}>;
export const GetPlaybackQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"GetPlayback"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"playback"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"playable"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"__typename"}},{"kind":"InlineFragment","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"Node"}}},{"kind":"InlineFragment","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"title"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"Displayable"}}},{"kind":"InlineFragment","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"durationMs"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"Track"}}}]}}]}}]}}],"kind":"Document"} as unknown as DocumentNode<GetPlaybackQuery, GetPlaybackQueryVariables>;
