export type EmitExtensions = 'none' | 'ts' | 'dts' | 'js';

/**
 * A single entry in the manifest, representing a GraphQL operation
 * that has been processed by codegen.
 */
export interface ManifestEntry {
  /** The raw GraphQL source (query, mutation, subscription, or fragment) */
  source: string;
  /** The relative path to the generated codegen file (without extension) */
  path: string;
  /** The exported name of the document in the codegen file */
  name: string;
}

/**
 * Configuration options for the @graphox/babel-plugin
 */
export interface GraphoxBabelPluginOptions {
  /**
   * Path to a JSON file containing the manifest entries.
   * Alternative to manifestData.
   */
  manifestPath?: string;
  /**
   * Inline manifest data containing GraphQL operations and their codegen paths.
   * Either manifestPath or manifestData must be provided.
   */
  manifestData?: ManifestEntry[];
  /**
   * The output directory where codegen files are located.
   * This is required and used to resolve relative import paths.
   */
  outputDir: string;
  /**
   * Additional import paths that should be treated as the GraphQL entrypoint.
   * By default, the plugin recognizes './graphql' and './index' paths relative to outputDir.
   * Use this for custom import aliases (e.g., '#graphql/graphql').
   */
  graphqlImportPaths?: string[];
  /**
   * File extension to append to generated import paths.
   * - "none" (default): No extension appended
   * - "ts": Append .ts
   * - "dts": Append .d.ts
   * - "js": Append .js
   */
  emitExtensions?: EmitExtensions;
}

