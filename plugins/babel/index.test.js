import { describe, it, expect } from 'vitest';
import * as babel from '@babel/core';
import path from 'path';
import plugin from './index.js';

function transform(code, options, filename = 'test.ts') {
  const result = babel.transformSync(code, {
    plugins: [[plugin, options]],
    filename: path.resolve(filename),
    babelrc: false,
    configFile: false,
  });
  return result.code;
}

describe('@soundtrack/graphox-babel', () => {
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
    expect(output).not.toContain('import { graphql }');
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

  it('handles mixed imports correctly', () => {
    const code = "import { graphql, other } from './gen/graphql'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, defaultOptions);

    expect(output).toContain('import { MyQueryDocument } from "./gen/query.codegen";');
    expect(output).toMatch(/import \{ other \} from ['"].\/gen\/graphql['"]/);
    expect(output).not.toContain('graphql,');
    expect(output).not.toContain(', graphql');
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

    // /root/gen/src/query.codegen relative to /root/app/ is ../gen/src/query.codegen
    expect(output).toContain('import { MyQueryDocument } from "../gen/src/query.codegen";');
  });

  it('supports gql tag', () => {
    const code = "import { gql } from './gen/graphql'; const q = gql(`query { me { id } }`);";
    const output = transform(code, defaultOptions);

    expect(output).toContain('import { MyQueryDocument } from "./gen/query.codegen";');
    expect(output).toContain('const q = MyQueryDocument;');
    expect(output).not.toContain('import { gql }');
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

  it('supports subpath imports (#graphql)', () => {
    const code = "import { graphql } from '#graphql/graphql'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, defaultOptions);

    expect(output).toContain('import { MyQueryDocument } from "./gen/query.codegen";');
    expect(output).toContain('const q = MyQueryDocument;');
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
});
