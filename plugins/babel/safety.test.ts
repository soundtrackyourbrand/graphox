import { describe, it, expect } from 'vitest';
import * as babel from '@babel/core';
import path from 'path';
import plugin from './index.js';

function transform(code: string, options: any, filename = 'test.ts') {
  const result = babel.transformSync(code, {
    plugins: [[plugin, options]],
    presets: ['@babel/preset-typescript'],
    filename: path.resolve(filename),
    babelrc: false,
    configFile: false,
  })!;
  return result.code;
}

describe('@graphox/babel-plugin safety', () => {
  it('handles document name clashing with the variable it is assigned to', () => {
    const manifest = [
      {
        source: 'query q { me { id } }',
        path: './q.codegen',
        name: 'q',
      },
    ];
    const options = { manifestData: manifest, outputDir: './gen' };
    const code = "import { graphql } from './gen/graphql'; const q = graphql(`query q { me { id } }`);";

    const output = transform(code, options);

    // Should NOT be "const q = q;"
    // Should be something like:
    // import { q as _q } from "./gen/q.codegen";
    // const q = _q;

    expect(output).not.toContain('const q = q;');
    expect(output).toMatch(/const q = _?q\d*;/);
    expect(output).toMatch(/import \{ q as _?q\d* \} from/);
  });

  it('handles document name clashing with an existing binding in scope', () => {
    const manifest = [
      {
        source: 'query MyQuery { me { id } }',
        path: './MyQuery.codegen',
        name: 'MyQuery',
      },
    ];
    const options = { manifestData: manifest, outputDir: './gen' };
    const code = "import { graphql } from './gen/graphql'; function test(MyQuery) { return graphql(`query MyQuery { me { id } }`); }";

    const output = transform(code, options);

    // The inner MyQuery refers to the argument.
    // The transformed code must NOT use the name MyQuery for the document if it's shadowed.
    expect(output).not.toContain('return MyQuery;');
    expect(output).toMatch(/return _?MyQuery\d*;/);
  });

  it('uses the same unique name for multiple usages of the same document', () => {
    const manifest = [
      {
        source: 'query q { me { id } }',
        path: './q.codegen',
        name: 'q',
      },
    ];
    const options = { manifestData: manifest, outputDir: './gen' };
    const code = "import { graphql } from './gen/graphql'; const q = graphql(`query q { me { id } }`); const another = graphql(`query q { me { id } }`);";

    const output = transform(code, options)!;

    const lines = output.split('\n');
    const importLine = lines.find(l => l.includes('import'))!;
    const match = importLine.match(/import \{ q as (_?q\d*) \}/)!;
    expect(match).toBeTruthy();
    const uniqueName = match[1];

    expect(output).toContain(`const q = ${uniqueName};`);
    expect(output).toContain(`const another = ${uniqueName};`);
  });
});
