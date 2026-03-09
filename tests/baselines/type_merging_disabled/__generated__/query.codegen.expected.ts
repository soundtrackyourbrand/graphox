/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

type Identity<T> = T extends object ? {} & { [P in keyof T]: T[P] } : T;

import type { TypedDocumentNode as DocumentNode } from "@graphql-typed-document-node/core";

export type Exact<T extends { [key: string]: unknown }> = { [K in keyof T]: T[K] };

export interface SearchQueryQuery {
  __typename: "Query";
  search: Array<{
      __typename: "Artist";
      id: string;
      name: string;
    }
    | {
      __typename: "Album";
      id: string;
      title: string;
    } | {
      __typename: "Playlist";
      id: string;
      title: string;
    } | {
      __typename: "Track";
      id: string;
      title: string;
    }>;
}

export type SearchQueryQueryVariables = Exact<{
}>;
export const SearchQueryQueryDocument = {"definitions":[{"kind":"OperationDefinition","name":{"kind":"Name","value":"SearchQuery"},"operation":"query","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"search"},"selectionSet":{"kind":"SelectionSet","selections":[{"kind":"InlineFragment","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"name"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"Artist"}}},{"kind":"InlineFragment","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"title"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"Album"}}},{"kind":"InlineFragment","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"title"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"Playlist"}}},{"kind":"InlineFragment","selectionSet":{"kind":"SelectionSet","selections":[{"kind":"Field","name":{"kind":"Name","value":"id"}},{"kind":"Field","name":{"kind":"Name","value":"title"}}]},"typeCondition":{"kind":"NamedType","name":{"kind":"Name","value":"Track"}}}]}}]}}],"kind":"Document"} as unknown as DocumentNode<SearchQueryQuery, SearchQueryQueryVariables>;
