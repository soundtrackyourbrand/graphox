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

    it('throws when a value import from graphql.ts cannot be rewritten', () => {
      const code = "import { graphql, other } from './gen/graphql'; const q = graphql(`query { me { id } }`); console.log(other);";

      expect(() => transform(code, defaultOptions)).toThrow(
        /could not rewrite "other" from "\.\/gen\/graphql"/,
      );
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

    it('throws when graphql() is missing from the manifest', () => {
      const code = "import { graphql } from './gen/graphql'; const q = graphql(`query Missing { me { id } }`);";

      expect(() => transform(code, defaultOptions)).toThrow(
        /could not find this graphql\(\) document in the manifest/i,
      );
    });

    it('throws when graphql() is not a single static string', () => {
      const code = `
        import { graphql } from './gen/graphql';
        const query = 'query { me { id } }';
        const q = graphql(query);
      `;

      expect(() => transform(code, defaultOptions)).toThrow(
        /could not statically analyze this graphql\(\) call/i,
      );
    });

    it('throws when a graphql import is still referenced after rewriting', () => {
      const code = `
        import { graphql } from './gen/graphql';
        const tag = graphql;
        console.log(tag);
      `;

      expect(() => transform(code, defaultOptions)).toThrow(
        /left a runtime reference to "graphql" after rewriting/i,
      );
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

    // The documents are gone, and what is left names the problem if anything
    // still calls it — a non-document would surface far from here.
    expect(output).toContain('throw new Error(');
    expect(output).toContain('graphql.ts');
    expect(output).toContain('export const gql = graphql;');
    expect(output).not.toContain('big map');
  });

  it('rewrites dynamic import destructuring from graphql.js to the codegen module', () => {
    const manifest = [
      {
        source: 'mutation CreateCart { createCart { id } }',
        path: './CreateCartMutation.codegen',
        name: 'CreateCartDocument',
      },
    ];
    const outputDir = path.resolve('/root/gen');
    const filename = '/root/app/TokenManager.ts';
    const output = transform(
      `
        async function load() {
          const { CreateCartDocument } = await import('../gen/graphql.js');
          return CreateCartDocument;
        }
      `,
      { manifestData: manifest, outputDir },
      filename,
    );

    expect(output).toContain('CreateCartDocument');
    expect(output).toContain('await import("../gen/CreateCartMutation.codegen")');
    expect(output).not.toContain('graphql.js');
  });

  it('rewrites multi-document dynamic imports across codegen files', () => {
    const manifest = [
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
    const output = transform(
      `
        async function load() {
          const { GetUserDocument, GetPostDocument } = await import('./gen/graphql.js');
          return [GetUserDocument, GetPostDocument];
        }
      `,
      { manifestData: manifest, outputDir: './gen' },
      'test.ts',
    );

    expect(output).toContain('Promise.all([import("./gen/user.codegen"), import("./gen/post.codegen")])');
    expect(output).toMatch(/GetUserDocument:\s*_graphoxModule\d*\.GetUserDocument/);
    expect(output).toMatch(/GetPostDocument:\s*_graphoxModule\d*\.GetPostDocument/);
    expect(output).not.toContain('graphql.js');
  });

  it('throws on unsupported dynamic import namespaces from the graphql entrypoint', () => {
    const manifest = [
      {
        source: 'query GetUser { user { id } }',
        path: './user.codegen',
        name: 'GetUserDocument',
      },
    ];
    const code = `
      async function load() {
        const docs = await import('./gen/graphql.js');
        return docs.GetUserDocument;
      }
    `;

    expect(() => transform(code, { manifestData: manifest, outputDir: './gen' })).toThrow(
      /could not fully rewrite this dynamic import from "\.\/gen\/graphql\.js"/,
    );
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

    it('throws on non-document imports alongside rewritten document imports', () => {
      const code = "import { GetUserDocument, SomeOtherType } from './gen/graphql';";

      expect(() => transform(code, reExportOptions)).toThrow(
        /could not rewrite "SomeOtherType" from "\.\/gen\/graphql"/,
      );
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

  describe('multi-project outputs', () => {
    const businessOut = path.resolve('/repo/apps/web/app/graphql');
    const remoteOut = path.resolve('/repo/packages/checkout/graphql');

    const outputs = [
      {
        outputDir: businessOut,
        importAlias: '@example/web/graphql',
        packageRoot: path.resolve('/repo/apps/web'),
        manifestData: [
          {
            source: 'mutation SetPrice { setPrice { id } }',
            path: './web.codegen',
            name: 'SetPriceMutationDocument',
          },
          {
            source: 'mutation ArchiveItem { archiveItem { id } }',
            path: './web.codegen',
            name: 'ArchiveItemMutationDocument',
          },
        ],
      },
      {
        outputDir: remoteOut,
        importAlias: '@example/checkout/graphql',
        packageRoot: path.resolve('/repo/packages/checkout'),
        manifestData: [
          {
            source: 'mutation SetPrice { setPrice { id } }',
            path: './checkout.codegen',
            name: 'SetPriceMutationDocument',
          },
          {
            source: 'mutation ArchiveItem { archiveItem(remote: true) { id } }',
            path: './checkout.codegen',
            name: 'ArchiveItemMutationDocument',
          },
        ],
      },
    ];

    it('rewrites a cross-package document import through the alias', () => {
      const code =
        "import { ArchiveItemMutationDocument } from '@example/checkout/graphql';\nconst d = ArchiveItemMutationDocument;";
      const output = transform(code, { outputs }, '/repo/apps/web/app/thing.ts');

      expect(output).toContain('@example/checkout/graphql/checkout.codegen');
      expect(output).not.toContain("'@example/checkout/graphql'");
    });

    it('keeps a same-package document import relative', () => {
      const code =
        "import { ArchiveItemMutationDocument } from '@example/web/graphql';\nconst d = ArchiveItemMutationDocument;";
      const output = transform(code, { outputs }, '/repo/apps/web/app/thing.ts');

      expect(output).toContain('graphql/web.codegen');
      expect(output).not.toContain('@example/web/graphql/');
    });

    it('resolves identical document source per entrypoint', () => {
      const code =
        "import { graphql } from './graphql';\nconst m = graphql(`mutation SetPrice { setPrice { id } }`);";

      const business = transform(code, { outputs }, path.join(businessOut, 'consumer.ts'));
      expect(business).toContain('./web.codegen');
      expect(business).not.toContain('checkout.codegen');

      const remote = transform(code, { outputs }, path.join(remoteOut, 'consumer.ts'));
      expect(remote).toContain('./checkout.codegen');
      expect(remote).not.toContain('web.codegen');
    });

    it('resolves the same document name from both projects in one module', () => {
      const code =
        "import { ArchiveItemMutationDocument as B1 } from '@example/web/graphql';\n" +
        "import { ArchiveItemMutationDocument as B2 } from '@example/checkout/graphql';\n" +
        'const a = B1; const b = B2;';
      const output = transform(code, { outputs }, '/repo/apps/web/app/thing.ts');

      expect(output).toContain('graphql/web.codegen');
      expect(output).toContain('@example/checkout/graphql/checkout.codegen');
    });

    it('clears the entrypoint of every configured output', () => {
      for (const dir of [businessOut, remoteOut]) {
        const output = transform(
          'export const documents = { a: 1 }; export const graphql = () => documents;',
          { outputs },
          path.join(dir, 'graphql.ts')
        );
        expect(output).not.toContain('export const documents');
        expect(output).not.toContain('a: 1');
        expect(output).toContain('throw new Error(');
      }
    });

    it('keeps two bindings for one document name owned by two outputs', () => {
      // Two projects export the same document name. The local-name cache was
      // keyed by the name alone, so the second import overwrote the first and
      // every reference silently resolved to whichever output came last.
      const code =
        "import { ArchiveItemMutationDocument as B1 } from '@example/web/graphql';\n" +
        "import { ArchiveItemMutationDocument } from '@example/checkout/graphql';\n" +
        'export const a = B1;\nexport const b = ArchiveItemMutationDocument;';

      const output = transform(code, { outputs }, '/repo/apps/other/thing.ts');

      expect(output).toContain('@example/web/graphql/web.codegen');
      expect(output).toContain('@example/checkout/graphql/checkout.codegen');
      expect(output).toContain('export const a = B1;');
      expect(output).toContain('export const b = ArchiveItemMutationDocument;');
    });

    it('names the configured outputs when a document is in no manifest', () => {
      const code =
        "import { MissingDoc } from '@example/web/graphql';\nconst d = MissingDoc;";
      expect(() => transform(code, { outputs }, '/repo/apps/web/app/thing.ts')).toThrow(
        /in none of the configured manifests/
      );
    });

    it('rejects overlapping outputDirs', () => {
      expect(() =>
        transform(
          'const x = 1;',
          { outputs: [{ outputDir: businessOut }, { outputDir: path.join(businessOut, 'nested') }] },
          '/repo/apps/web/app/thing.ts'
        )
      ).toThrow(/overlap/);
    });

    it('still accepts the single-output form', () => {
      const code =
        "import { graphql } from './gen/graphql';\nconst q = graphql(`query { me { id } }`);";
      const output = transform(code, defaultOptions);
      expect(output).toContain('./gen/query.codegen');
    });
  });

  describe('re-exports of documents', () => {
    const reexportOptions = {
      outputDir: './gen',
      manifestData: [
        { source: 'query { me { id } }', path: './query.codegen', name: 'MyQueryDocument' },
        { source: 'fragment F on User { id }', path: './other.codegen', name: 'FFragmentDoc' },
      ],
    };

    it('points a document re-export at the generated file', () => {
      // The entrypoint is emptied in its own compilation, so a re-export left
      // pointing at it resolves to nothing — a barrel that silently exports
      // undefined, which no type check or bundler treats as an error.
      const output = transform(
        "export { MyQueryDocument } from './gen/graphql';",
        reexportOptions
      );

      expect(output).toContain('export { MyQueryDocument } from "./gen/query.codegen"');
      expect(output).not.toContain('./gen/graphql');
    });

    it('splits a re-export by generated file', () => {
      const output = transform(
        "export { MyQueryDocument as Q, FFragmentDoc } from './gen/graphql';",
        reexportOptions
      );

      expect(output).toContain('export { MyQueryDocument as Q } from "./gen/query.codegen"');
      expect(output).toContain('export { FFragmentDoc } from "./gen/other.codegen"');
    });

    it('drops a type-only re-export', () => {
      const output = transform(
        "export type { MyQueryDocument } from './gen/graphql';\nexport const x = 1;",
        reexportOptions
      );

      expect(output).not.toContain('./gen/graphql');
      expect(output).toContain('export const x = 1');
    });

    it('rejects a star re-export of an entrypoint', () => {
      expect(() => transform("export * from './gen/graphql';", reexportOptions)).toThrow(
        /star re-export/
      );
    });

    it('rejects a namespace re-export of an entrypoint', () => {
      expect(() =>
        transform("export * as all from './gen/graphql';", reexportOptions)
      ).toThrow(/star re-export/);
    });

    it('rejects re-exporting the tag', () => {
      expect(() => transform("export { graphql } from './gen/graphql';", reexportOptions)).toThrow(
        /does not exist at runtime/
      );
    });
  });

  it('keeps new imports where the ones they replace stood', () => {
    // Hoisting them to the top puts the generated file's module-init work ahead
    // of a side-effect import that was written to run first.
    const output = transform(
      "import './polyfill';\nimport { graphql } from './gen/graphql';\nconst q = graphql(`query { me { id } }`);",
      defaultOptions
    );

    expect(output.indexOf('./polyfill')).toBeLessThan(output.indexOf('./gen/query.codegen'));
  });

  it('keeps documents that differ only inside a string literal distinct', () => {
    // Two anonymous queries, identical but for a string argument. Neither has a
    // name, so nothing rejects the pair, and stripping whitespace everywhere used
    // to collapse them onto one manifest key.
    const output = transform(
      'import { graphql } from \'./gen/graphql\'; const q = graphql(`query { search(term: "ab") { id } }`);',
      {
        outputDir: './gen',
        manifestData: [
          {
            source: 'query { search(term: "a b") { id } }',
            path: './spaced.codegen',
            name: 'SpacedDocument',
          },
          {
            source: 'query { search(term: "ab") { id } }',
            path: './tight.codegen',
            name: 'TightDocument',
          },
        ],
      }
    );

    expect(output).toContain('./gen/tight.codegen');
    expect(output).not.toContain('spaced.codegen');
  });

});
