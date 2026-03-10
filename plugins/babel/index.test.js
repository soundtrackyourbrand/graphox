import { describe, it, expect } from 'vitest';
import * as babel from '@babel/core';
import path from 'path';
import plugin from './index.js';

function transform(code, options, filename = 'test.ts') {
  const result = babel.transformSync(code, {
    plugins: [[plugin, options]],
    presets: ['@babel/preset-typescript'],
    filename: path.resolve(filename),
    babelrc: false,
    configFile: false,
  });
  return result.code;
}

describe('@graphox/babel-plugin', () => {
  const defaultManifest = [
    {
      source: 'query { me { id } }',
      path: './query.codegen',
      name: 'MyQueryDocument',
    },
  ];

  const defaultOptions = {
    manifestData: defaultManifest,
    outputDir: './gen',
  };

  it('transforms basic graphql call and removes import', () => {
    const code = "import { graphql } from './gen/graphql'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, defaultOptions);

    expect(output).toContain('import { MyQueryDocument } from "./gen/query.codegen";');
    expect(output).toContain('const q = MyQueryDocument;');
    expect(output).not.toContain('from "./gen/graphql"');
  });

  it('transforms multiple calls', () => {
    const manifest = [
      {
        source: 'query GetMe { me { id } }',
        path: './me.codegen',
        name: 'GetMeDocument',
      },
      {
        source: 'query GetOther { other { id } }',
        path: './other.codegen',
        name: 'GetOtherDocument',
      },
    ];
    const options = { manifestData: manifest, outputDir: './gen' };
    const code = `
      import { graphql } from './gen/graphql';
      const q1 = graphql(\`query GetMe { me { id } }\`);
      const q2 = graphql(\`query GetOther { other { id } }\`);
    `;
    const output = transform(code, options);

    expect(output).toContain('import { GetMeDocument } from "./gen/me.codegen";');
    expect(output).toContain('import { GetOtherDocument } from "./gen/other.codegen";');
    expect(output).toContain('const q1 = GetMeDocument;');
    expect(output).toContain('const q2 = GetOtherDocument;');
  });

  it('removes all imports from graphql.ts including unknown specifiers', () => {
    const code = "import { graphql, other } from './gen/graphql'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, defaultOptions);

    expect(output).toContain('import { MyQueryDocument } from "./gen/query.codegen";');
    expect(output).not.toContain('from "./gen/graphql"');
    expect(output).not.toContain('graphql,');
    expect(output).not.toContain(', graphql');
    expect(output).not.toContain('other');
  });

  it('resolves relative paths correctly', () => {
    const manifest = [
      {
        source: 'query { me { id } }',
        path: './src/query.codegen',
        name: 'MyQueryDocument',
      },
    ];
    const outputDir = path.resolve('/root/gen');
    const options = { manifestData: manifest, outputDir };
    const filename = '/root/app/test.ts';

    const code = "import { graphql } from '../gen/graphql'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, options, filename);

    expect(output).toContain('import { MyQueryDocument } from "../gen/src/query.codegen";');
  });

  it('resolves absolute import paths against outputDir', () => {
    const outputDir = path.resolve('/root/gen');
    const options = { manifestData: defaultManifest, outputDir };
    const filename = '/root/app/test.ts';
    const code = "import { graphql } from '/root/gen/graphql.ts'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, options, filename);

    expect(output).toContain('import { MyQueryDocument } from "../gen/query.codegen";');
    expect(output).toContain('const q = MyQueryDocument;');
    expect(output).not.toContain("from '/root/gen/graphql.ts'");
  });

  it('supports gql tag', () => {
    const code = "import { gql } from './gen/graphql'; const q = gql(`query { me { id } }`);";
    const output = transform(code, defaultOptions);

    expect(output).toContain('import { MyQueryDocument } from "./gen/query.codegen";');
    expect(output).toContain('const q = MyQueryDocument;');
    expect(output).not.toContain('from "./gen/graphql"');
  });

  it('is whitespace insensitive (normalization)', () => {
    const code = `
      import { graphql } from './gen/graphql';
      const q = graphql(\`query {
        me {
          id
        }
      }\`);
    `;
    const output = transform(code, defaultOptions);
    expect(output).toContain('const q = MyQueryDocument;');
  });

  it('does not transform identifiers from other libraries', () => {
    const code = "import { graphql } from 'other-lib'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, defaultOptions);

    expect(output).toContain("import { graphql } from 'other-lib';");
    expect(output).toContain('graphql(');
    expect(output).not.toContain('MyQueryDocument');
  });

  it('supports configured subpath imports (#graphql)', () => {
    const options = {
      ...defaultOptions,
      graphqlImportPaths: ['#graphql/graphql'],
    };
    const code = "import { graphql } from '#graphql/graphql'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, options);

    expect(output).toContain('import { MyQueryDocument } from "./gen/query.codegen";');
    expect(output).toContain('const q = MyQueryDocument;');
  });

  it('does not transform unrelated aliases containing graphql', () => {
    const code = "import { graphql } from '#app/graphql/gql'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, defaultOptions);

    expect(output).toContain("import { graphql } from '#app/graphql/gql';");
    expect(output).toContain('graphql(');
    expect(output).not.toContain('MyQueryDocument');
  });

  it('supports explicit import paths', () => {
    const options = {
      ...defaultOptions,
      graphqlImportPaths: ['@app/gql-entrypoint'],
    };
    const code = "import { graphql } from '@app/gql-entrypoint'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, options);

    expect(output).toContain('import { MyQueryDocument } from "./gen/query.codegen";');
    expect(output).toContain('const q = MyQueryDocument;');
  });

  it('clears the entrypoint file', () => {
    const outputDir = path.resolve('./gen');
    const filename = path.join(outputDir, 'graphql.ts');
    const code = "export const graphql = () => { /* big map */ }; export const gql = graphql;";
    const output = transform(code, { outputDir }, filename);

    expect(output).toContain('export const graphql = () => null;');
    expect(output).toContain('export const gql = graphql;');
    expect(output).not.toContain('big map');
  });

  describe('emit extensions', () => {
    it('appends .ts extension when emitExtensions is "ts"', () => {
      const code = "import { graphql } from './gen/graphql'; const q = graphql(`query { me { id } }`);";
      const output = transform(code, { ...defaultOptions, emitExtensions: 'ts' });

      expect(output).toContain('import { MyQueryDocument } from "./gen/query.codegen.ts";');
    });

    it('appends .js extension when emitExtensions is "js"', () => {
      const code = "import { graphql } from './gen/graphql'; const q = graphql(`query { me { id } }`);";
      const output = transform(code, { ...defaultOptions, emitExtensions: 'js' });

      expect(output).toContain('import { MyQueryDocument } from "./gen/query.codegen.js";');
    });

    it('appends .d.ts extension when emitExtensions is "dts"', () => {
      const code = "import { graphql } from './gen/graphql'; const q = graphql(`query { me { id } }`);";
      const output = transform(code, { ...defaultOptions, emitExtensions: 'dts' });

      expect(output).toContain('import { MyQueryDocument } from "./gen/query.codegen.d.ts";');
    });

    it('does not append extension when emitExtensions is "none" or omitted', () => {
      const code = "import { graphql } from './gen/graphql'; const q = graphql(`query { me { id } }`);";
      const output1 = transform(code, { ...defaultOptions, emitExtensions: 'none' });
      const output2 = transform(code, defaultOptions);

      expect(output1).toContain('import { MyQueryDocument } from "./gen/query.codegen";');
      expect(output2).toContain('import { MyQueryDocument } from "./gen/query.codegen";');
    });

    it('appends extension to relative paths correctly', () => {
      const manifest = [
        {
          source: 'query { me { id } }',
          path: './src/query.codegen',
          name: 'MyQueryDocument',
        },
      ];
      const outputDir = path.resolve('/root/gen');
      const options = { manifestData: manifest, outputDir, emitExtensions: 'js' };
      const filename = '/root/app/test.ts';

      const code = "import { graphql } from '../gen/graphql'; const q = graphql(`query { me { id } }`);";
      const output = transform(code, options, filename);

      expect(output).toContain('import { MyQueryDocument } from "../gen/src/query.codegen.js";');
    });
  });

  describe('re-exported document imports', () => {
    const reExportManifest = [
      {
        source: 'query GetUser { user { id } }',
        path: './user.codegen',
        name: 'GetUserDocument',
      },
      {
        source: 'query GetPost { post { id } }',
        path: './post.codegen',
        name: 'GetPostDocument',
      },
    ];

    const reExportOptions = {
      manifestData: reExportManifest,
      outputDir: './gen',
    };

    it('rewrites single document name import from graphql entrypoint', () => {
      const code = "import { GetUserDocument } from './gen/graphql'; console.log(GetUserDocument);";
      const output = transform(code, reExportOptions);

      expect(output).toContain("import { GetUserDocument } from \"./gen/user.codegen\";");
      expect(output).not.toContain("from './gen/graphql'");
      expect(output).toContain('console.log(GetUserDocument);');
    });

    it('rewrites multiple document name imports to correct codegen files', () => {
      const code = "import { GetUserDocument, GetPostDocument } from './gen/graphql';";
      const output = transform(code, reExportOptions);

      expect(output).toContain("import { GetUserDocument } from \"./gen/user.codegen\";");
      expect(output).toContain("import { GetPostDocument } from \"./gen/post.codegen\";");
      expect(output).not.toContain("from './gen/graphql'");
    });

    it('removes graphql import alongside document imports', () => {
      const code = "import { GetUserDocument, graphql } from './gen/graphql';";
      const output = transform(code, reExportOptions);

      expect(output).toContain("import { GetUserDocument } from \"./gen/user.codegen\";");
      expect(output).not.toContain("from './gen/graphql'");
      expect(output).not.toContain('graphql');
    });

    it('removes non-document imports while rewriting document imports', () => {
      const code = "import { GetUserDocument, SomeOtherType } from './gen/graphql';";
      const output = transform(code, reExportOptions);

      expect(output).toContain("import { GetUserDocument } from \"./gen/user.codegen\";");
      expect(output).not.toContain("from './gen/graphql'");
      expect(output).not.toContain('SomeOtherType');
    });

    it('removes type-only imports (they dont exist in minified JS)', () => {
      const code = "import type { GetUserDocument } from './gen/graphql';";
      const output = transform(code, reExportOptions);

      // Type-only imports are removed entirely since they don't exist in minified JS
      expect(output).not.toContain("from './gen/graphql'");
      expect(output).not.toContain('GetUserDocument');
    });

    it('removes inline type specifiers from mixed imports', () => {
      const code = "import { GetUserDocument, type GetPostDocument } from './gen/graphql';";
      const output = transform(code, reExportOptions);

      // Only GetUserDocument (non-type) is kept and rewritten
      expect(output).toContain("import { GetUserDocument } from \"./gen/user.codegen\";");
      // GetPostDocument with inline type is removed
      expect(output).not.toContain("GetPostDocument");
      expect(output).not.toContain("from './gen/graphql'");
    });

    it('rewrites imports from index.ts barrel file', () => {
      const code = "import { GetUserDocument } from './gen/index';";
      const output = transform(code, reExportOptions);

      expect(output).toContain("import { GetUserDocument } from \"./gen/user.codegen\";");
      expect(output).not.toContain("from './gen/index'");
    });

    it('rewrites imports from index.ts with extension', () => {
      const code = "import { GetUserDocument } from './gen/index.ts';";
      const output = transform(code, reExportOptions);

      expect(output).toContain("import { GetUserDocument } from \"./gen/user.codegen\";");
      expect(output).not.toContain("from './gen/index");
    });

    it('handles aliased document imports', () => {
      const code = "import { GetUserDocument as MyUserDoc } from './gen/graphql'; console.log(MyUserDoc);";
      const output = transform(code, reExportOptions);

      expect(output).toContain("import { GetUserDocument as MyUserDoc } from \"./gen/user.codegen\";");
      expect(output).toContain('console.log(MyUserDoc);');
    });

    it('rewrites fragment document imports when generate_ast_for_fragments is enabled', () => {
      const manifest = [
        {
          source: 'query GetUser { user { id } }',
          path: './user.codegen',
          name: 'GetUserDocument',
        },
        {
          source: 'fragment UserFields on User { id name }',
          path: './userFields.codegen',
          name: 'UserFieldsFragmentDocument',
        },
      ];
      const options = { manifestData: manifest, outputDir: './gen' };
      const code = "import { GetUserDocument, UserFieldsFragmentDocument } from './gen/graphql';";
      const output = transform(code, options);

      expect(output).toContain("import { GetUserDocument } from \"./gen/user.codegen\";");
      expect(output).toContain("import { UserFieldsFragmentDocument } from \"./gen/userFields.codegen\";");
      expect(output).not.toContain("from './gen/graphql'");
    });

    it('rewrites imports from directory resolving to index', () => {
      const code = "import { GetUserDocument } from './gen';";
      const output = transform(code, reExportOptions);

      expect(output).toContain("import { GetUserDocument } from \"./gen/user.codegen\";");
      expect(output).not.toContain("from './gen'");
    });

    it('rewrites graphql function import from index.ts barrel', () => {
      const code = "import { graphql } from './gen/index'; const q = graphql(`query GetUser { user { id } }`);";
      const output = transform(code, reExportOptions);

      expect(output).toContain("import { GetUserDocument } from \"./gen/user.codegen\";");
      expect(output).toContain("const q = GetUserDocument;");
      expect(output).not.toContain("from './gen/index'");
      expect(output).not.toContain('graphql');
    });

    it('rewrites gql function import from index.ts barrel', () => {
      const code = "import { gql } from './gen/index'; const q = gql(`query GetUser { user { id } }`);";
      const output = transform(code, reExportOptions);

      expect(output).toContain("import { GetUserDocument } from \"./gen/user.codegen\";");
      expect(output).toContain("const q = GetUserDocument;");
      expect(output).not.toContain("from './gen/index'");
      expect(output).not.toContain('gql');
    });

    it('prefers the operation document when manifest entries share the same source', () => {
      const source = `
        query MusicRouteQuery($playlistId: ID!, $market: IsoCountry!, $categoryTypes: [String!]) {
          playlist(id: $playlistId) {
            ...SourceViewPlaylist
            ...Playlist_MusicRouteMeta
            ...BrowseCategories
          }
        }

        fragment Playlist_MusicRouteMeta on Playlist {
          id
          permissions
          name
          description
          snapshot
          updatedAt
          ...Displayable
          trackStatistics(market: $market) {
            total
          }
        }

        fragment BrowseCategories on Playlist {
          id
          permissions
          browseCategories(categoryTypes: $categoryTypes) {
            id
            name
            slug
            type
          }
        }
      `;
      const manifest = [
        {
          source,
          path: './music.codegen',
          name: 'MusicRouteQueryQueryDocument',
        },
        {
          source,
          path: './music.codegen',
          name: 'Playlist_MusicRouteMetaFragmentDoc',
        },
        {
          source,
          path: './music.codegen',
          name: 'BrowseCategoriesFragmentDoc',
        },
      ];
      const options = { manifestData: manifest, outputDir: './gen' };
      const code = `
        import { graphql } from './gen/graphql';
        const MusicRouteQuery = graphql(/* GraphQL */ \`${source}\`);
      `;
      const output = transform(code, options);

      expect(output).toContain(
        'import { MusicRouteQueryQueryDocument } from "./gen/music.codegen";',
      );
      expect(output).toContain('const MusicRouteQuery = MusicRouteQueryQueryDocument;');
      expect(output).not.toContain('Playlist_MusicRouteMetaFragmentDoc');
      expect(output).not.toContain('BrowseCategoriesFragmentDoc');
    });

    it('keeps the first fragment document for fragment-only shared sources', () => {
      const source = `
        fragment PlaylistFields on Playlist {
          id
        }

        fragment PlaylistPermissions on Playlist {
          permissions
        }
      `;
      const manifest = [
        {
          source,
          path: './playlist.codegen',
          name: 'PlaylistFieldsFragmentDoc',
        },
        {
          source,
          path: './playlist.codegen',
          name: 'PlaylistPermissionsFragmentDoc',
        },
      ];
      const options = { manifestData: manifest, outputDir: './gen' };
      const code = `
        import { graphql } from './gen/graphql';
        const PlaylistFields = graphql(/* GraphQL */ \`${source}\`);
      `;
      const output = transform(code, options);

      expect(output).toContain(
        'import { PlaylistFieldsFragmentDoc } from "./gen/playlist.codegen";',
      );
      expect(output).toContain('const PlaylistFields = PlaylistFieldsFragmentDoc;');
      expect(output).not.toContain('PlaylistPermissionsFragmentDoc');
    });
  });
});
