/* tslint:disable */
/* eslint-disable */
// This file was automatically generated and should not be edited.

import { FieldPolicy, FieldReadFunction, TypePolicies, TypePolicy } from '@apollo/client/cache';

export type AlbumKeySpecifier = ('id' | 'title' | 'artist' | AlbumKeySpecifier)[];
export type AlbumFieldPolicy = {
  id?: FieldPolicy<any> | FieldReadFunction<any>,
  title?: FieldPolicy<any> | FieldReadFunction<any>,
  artist?: FieldPolicy<any> | FieldReadFunction<any>,
};

export type ArtistKeySpecifier = ('id' | 'name' | ArtistKeySpecifier)[];
export type ArtistFieldPolicy = {
  id?: FieldPolicy<any> | FieldReadFunction<any>,
  name?: FieldPolicy<any> | FieldReadFunction<any>,
};

export type AudioKeySpecifier = ('duration' | AudioKeySpecifier)[];
export type AudioFieldPolicy = {
  duration?: FieldPolicy<any> | FieldReadFunction<any>,
};

export type AudioBookKeySpecifier = ('id' | 'title' | 'duration' | AudioBookKeySpecifier)[];
export type AudioBookFieldPolicy = {
  id?: FieldPolicy<any> | FieldReadFunction<any>,
  title?: FieldPolicy<any> | FieldReadFunction<any>,
  duration?: FieldPolicy<any> | FieldReadFunction<any>,
};

export type AudioFileKeySpecifier = ('id' | 'duration' | 'url' | AudioFileKeySpecifier)[];
export type AudioFileFieldPolicy = {
  id?: FieldPolicy<any> | FieldReadFunction<any>,
  duration?: FieldPolicy<any> | FieldReadFunction<any>,
  url?: FieldPolicy<any> | FieldReadFunction<any>,
};

export type DisplayableKeySpecifier = ('title' | DisplayableKeySpecifier)[];
export type DisplayableFieldPolicy = {
  title?: FieldPolicy<any> | FieldReadFunction<any>,
};

export type MovieKeySpecifier = ('id' | 'title' | 'thumbnail' | MovieKeySpecifier)[];
export type MovieFieldPolicy = {
  id?: FieldPolicy<any> | FieldReadFunction<any>,
  title?: FieldPolicy<any> | FieldReadFunction<any>,
  thumbnail?: FieldPolicy<any> | FieldReadFunction<any>,
};

export type NodeKeySpecifier = ('id' | NodeKeySpecifier)[];
export type NodeFieldPolicy = {
  id?: FieldPolicy<any> | FieldReadFunction<any>,
};

export type QueryKeySpecifier = ('search' | 'node' | QueryKeySpecifier)[];
export type QueryFieldPolicy = {
  search?: FieldPolicy<any> | FieldReadFunction<any>,
  node?: FieldPolicy<any> | FieldReadFunction<any>,
};

export type TrackKeySpecifier = ('id' | 'title' | 'duration' | TrackKeySpecifier)[];
export type TrackFieldPolicy = {
  id?: FieldPolicy<any> | FieldReadFunction<any>,
  title?: FieldPolicy<any> | FieldReadFunction<any>,
  duration?: FieldPolicy<any> | FieldReadFunction<any>,
};

export type VideoKeySpecifier = ('thumbnail' | VideoKeySpecifier)[];
export type VideoFieldPolicy = {
  thumbnail?: FieldPolicy<any> | FieldReadFunction<any>,
};

export type VideoFileKeySpecifier = ('id' | 'thumbnail' | 'url' | VideoFileKeySpecifier)[];
export type VideoFileFieldPolicy = {
  id?: FieldPolicy<any> | FieldReadFunction<any>,
  thumbnail?: FieldPolicy<any> | FieldReadFunction<any>,
  url?: FieldPolicy<any> | FieldReadFunction<any>,
};

export type StrictTypedTypePolicies = {
  Album?: Omit<TypePolicy, "fields" | "keyFields"> & {
    keyFields?: false | AlbumKeySpecifier | (() => undefined | AlbumKeySpecifier),
    fields?: AlbumFieldPolicy,
  },
  Artist?: Omit<TypePolicy, "fields" | "keyFields"> & {
    keyFields?: false | ArtistKeySpecifier | (() => undefined | ArtistKeySpecifier),
    fields?: ArtistFieldPolicy,
  },
  Audio?: Omit<TypePolicy, "fields" | "keyFields"> & {
    keyFields?: false | AudioKeySpecifier | (() => undefined | AudioKeySpecifier),
    fields?: AudioFieldPolicy,
  },
  AudioBook?: Omit<TypePolicy, "fields" | "keyFields"> & {
    keyFields?: false | AudioBookKeySpecifier | (() => undefined | AudioBookKeySpecifier),
    fields?: AudioBookFieldPolicy,
  },
  AudioFile?: Omit<TypePolicy, "fields" | "keyFields"> & {
    keyFields?: false | AudioFileKeySpecifier | (() => undefined | AudioFileKeySpecifier),
    fields?: AudioFileFieldPolicy,
  },
  Displayable?: Omit<TypePolicy, "fields" | "keyFields"> & {
    keyFields?: false | DisplayableKeySpecifier | (() => undefined | DisplayableKeySpecifier),
    fields?: DisplayableFieldPolicy,
  },
  Movie?: Omit<TypePolicy, "fields" | "keyFields"> & {
    keyFields?: false | MovieKeySpecifier | (() => undefined | MovieKeySpecifier),
    fields?: MovieFieldPolicy,
  },
  Node?: Omit<TypePolicy, "fields" | "keyFields"> & {
    keyFields?: false | NodeKeySpecifier | (() => undefined | NodeKeySpecifier),
    fields?: NodeFieldPolicy,
  },
  Query?: Omit<TypePolicy, "fields" | "keyFields"> & {
    keyFields?: false | QueryKeySpecifier | (() => undefined | QueryKeySpecifier),
    fields?: QueryFieldPolicy,
  },
  Track?: Omit<TypePolicy, "fields" | "keyFields"> & {
    keyFields?: false | TrackKeySpecifier | (() => undefined | TrackKeySpecifier),
    fields?: TrackFieldPolicy,
  },
  Video?: Omit<TypePolicy, "fields" | "keyFields"> & {
    keyFields?: false | VideoKeySpecifier | (() => undefined | VideoKeySpecifier),
    fields?: VideoFieldPolicy,
  },
  VideoFile?: Omit<TypePolicy, "fields" | "keyFields"> & {
    keyFields?: false | VideoFileKeySpecifier | (() => undefined | VideoFileKeySpecifier),
    fields?: VideoFileFieldPolicy,
  }
};

export type TypedTypePolicies = StrictTypedTypePolicies & TypePolicies;
