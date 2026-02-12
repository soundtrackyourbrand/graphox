/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

export interface Album {
  __typename: "Album";
  artist?: Artist | null;
  id: string;
  title: string;
}

export interface Artist {
  __typename: "Artist";
  id: string;
  name: string;
}

export interface AudioBook {
  __typename: "AudioBook";
  duration: number;
  id: string;
  title: string;
}

export interface AudioFile {
  __typename: "AudioFile";
  duration: number;
  id: string;
  url: string;
}

export interface Movie {
  __typename: "Movie";
  id: string;
  thumbnail: string;
  title: string;
}

export interface Query {
  __typename: "Query";
  node?: Node | null;
  search?: Array<SearchResult | null> | null;
}

export interface Track {
  __typename: "Track";
  duration: number;
  id: string;
  title: string;
}

export interface VideoFile {
  __typename: "VideoFile";
  id: string;
  thumbnail: string;
  url: string;
}

export interface Audio {
  __typename: "AudioBook" | "AudioFile" | "Track";
  duration: number;
}

export interface Displayable {
  __typename: "Album" | "Track";
  title: string;
}

export interface Node {
  __typename: "Album" | "Artist" | "AudioBook" | "Movie" | "Track";
  id: string;
}

export interface Video {
  __typename: "Movie" | "VideoFile";
  thumbnail: string;
}

export type Media = AudioFile | VideoFile;

export type SearchResult = Album | Artist | Track;

