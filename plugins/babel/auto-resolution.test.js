import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import * as babel from '@babel/core';
import path from 'path';
import fs from 'fs';
import os from 'os';
import { execFileSync } from 'child_process';
import plugin from './index.js';

function transform(code, options, filename) {
  const result = babel.transformSync(code, {
    plugins: [[plugin, options]],
    presets: ['@babel/preset-typescript'],
    filename: filename ? path.resolve(filename) : 'test.ts',
    babelrc: false,
    configFile: false,
  });
  return result.code;
}

describe('@graphox/babel-plugin auto-resolution', () => {
  let tmpDir;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'graphox-babel-test-'));
  });

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  const manifestData = [
    {
      source: 'query { me { id } }',
      path: './query.codegen',
      name: 'MyQueryDocument',
    },
  ];

  it('automatically uses manifest.json in outputDir if manifestPath/Data is missing', () => {
    const outputDir = path.join(tmpDir, 'gen');
    fs.mkdirSync(outputDir, { recursive: true });
    fs.writeFileSync(path.join(outputDir, 'manifest.json'), JSON.stringify(manifestData));

    const code = "import { graphql } from './gen/graphql'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, { outputDir }, path.join(tmpDir, 'test.ts'));

    expect(output).toContain('import { MyQueryDocument } from "./gen/query.codegen";');
  });

  it('reads imports from package.json', () => {
    const outputDir = path.join(tmpDir, 'gen');
    fs.mkdirSync(outputDir, { recursive: true });
    
    // Create package.json with imports
    const pkgJson = {
      name: 'test-pkg',
      imports: {
        '#gql': './gen/graphql.ts'
      }
    };
    fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(pkgJson));
    
    fs.writeFileSync(path.join(outputDir, 'manifest.json'), JSON.stringify(manifestData));

    const code = "import { graphql } from '#gql'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, { outputDir }, path.join(tmpDir, 'src', 'test.ts'));

    expect(output).toContain('import { MyQueryDocument } from "../gen/query.codegen";');
  });

  it('reads a package.json imports wildcard that has a suffix after the star', () => {
    // "#gql/*" -> "./gen/*.ts": call sites write
    // `import { graphql } from '#gql/playback/graphql'`, so that exact specifier
    // is what has to be recognised. Resolving the target verbatim left an `*` in
    // the path, which matches nothing — the call site was then left alone while
    // the entrypoint it calls was emptied on its path match.
    const outputDir = path.join(tmpDir, 'gen', 'playback');
    fs.mkdirSync(outputDir, { recursive: true });

    fs.writeFileSync(
      path.join(tmpDir, 'package.json'),
      JSON.stringify({ name: 'test-pkg', imports: { '#gql/*': './gen/*.ts' } })
    );
    fs.writeFileSync(path.join(outputDir, 'manifest.json'), JSON.stringify(manifestData));

    const code =
      "import { graphql } from '#gql/playback/graphql'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, { outputDir }, path.join(tmpDir, 'src', 'test.ts'));

    expect(output).toContain('../gen/playback/query.codegen');
    expect(output).not.toContain('graphql(');
  });

  it('reads a tsconfig paths wildcard that has a suffix after the star', () => {
    const outputDir = path.join(tmpDir, 'gen', 'playback');
    fs.mkdirSync(outputDir, { recursive: true });

    fs.writeFileSync(
      path.join(tmpDir, 'tsconfig.json'),
      JSON.stringify({ compilerOptions: { baseUrl: '.', paths: { '@gen/*': ['gen/*.ts'] } } })
    );
    fs.writeFileSync(path.join(outputDir, 'manifest.json'), JSON.stringify(manifestData));

    const code =
      "import { graphql } from '@gen/playback/graphql'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, { outputDir }, path.join(tmpDir, 'src', 'test.ts'));

    expect(output).toContain('../gen/playback/query.codegen');
    expect(output).not.toContain('graphql(');
  });

  it('reads imports from package.json with object-form conditional exports', () => {
    const outputDir = path.join(tmpDir, 'gen');
    fs.mkdirSync(outputDir, { recursive: true });
    
    // Create package.json with object-form imports
    const pkgJson = {
      name: 'test-pkg',
      imports: {
        '#gql': {
          import: './gen/graphql.js',
          types: './gen/graphql.d.ts'
        }
      }
    };
    fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(pkgJson));
    
    fs.writeFileSync(path.join(outputDir, 'manifest.json'), JSON.stringify(manifestData));

    const code = "import { graphql } from '#gql'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, { outputDir }, path.join(tmpDir, 'src', 'test.ts'));

    expect(output).toContain('import { MyQueryDocument } from "../gen/query.codegen";');
  });

  it('handles null entries in package.json imports', () => {
    const outputDir = path.join(tmpDir, 'gen');
    fs.mkdirSync(outputDir, { recursive: true });
    
    // Create package.json with a null import entry
    const pkgJson = {
      name: 'test-pkg',
      imports: {
        '#gql': './gen/graphql.ts',
        '#null': null
      }
    };
    fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(pkgJson));
    
    fs.writeFileSync(path.join(outputDir, 'manifest.json'), JSON.stringify(manifestData));

    const code = "import { graphql } from '#gql'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, { outputDir }, path.join(tmpDir, 'src', 'test.ts'));

    expect(output).toContain('import { MyQueryDocument } from "../gen/query.codegen";');
  });

  it('reads paths from tsconfig.json', () => {
    const outputDir = path.join(tmpDir, 'gen');
    fs.mkdirSync(outputDir, { recursive: true });
    
    // Create tsconfig.json with paths
    const tsconfig = {
      compilerOptions: {
        baseUrl: '.',
        paths: {
          '@graphql': ['gen/graphql.ts']
        }
      }
    };
    fs.writeFileSync(path.join(tmpDir, 'tsconfig.json'), JSON.stringify(tsconfig));
    
    fs.writeFileSync(path.join(outputDir, 'manifest.json'), JSON.stringify(manifestData));

    const code = "import { graphql } from '@graphql'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, { outputDir }, path.join(tmpDir, 'src', 'test.ts'));

    expect(output).toContain('import { MyQueryDocument } from "../gen/query.codegen";');
  });
  
  it('reads paths from tsconfig.json with directory mapping', () => {
    const outputDir = path.join(tmpDir, 'gen');
    fs.mkdirSync(outputDir, { recursive: true });
    
    const tsconfig = {
      compilerOptions: {
        baseUrl: '.',
        paths: {
          '@graphql': ['gen']
        }
      }
    };
    fs.writeFileSync(path.join(tmpDir, 'tsconfig.json'), JSON.stringify(tsconfig));
    
    fs.writeFileSync(path.join(outputDir, 'manifest.json'), JSON.stringify(manifestData));

    const code = "import { graphql } from '@graphql/graphql'; const q = graphql(`query { me { id } }`);";
    
    const output = transform(code, { outputDir }, path.join(tmpDir, 'src', 'test.ts'));

    expect(output).toContain('import { MyQueryDocument } from "../gen/query.codegen";');
  });

  it('handles monorepo structure (searching up)', () => {
    const rootDir = tmpDir;
    const pkgDir = path.join(rootDir, 'packages', 'app');
    const outputDir = path.join(pkgDir, 'src', 'generated');
    
    fs.mkdirSync(path.join(pkgDir, 'src'), { recursive: true });
    fs.mkdirSync(outputDir, { recursive: true });
    
    // tsconfig in package root
    const tsconfig = {
      compilerOptions: {
        paths: {
          '~gql': ['./src/generated/graphql']
        }
      }
    };
    fs.writeFileSync(path.join(pkgDir, 'tsconfig.json'), JSON.stringify(tsconfig));
    fs.writeFileSync(path.join(outputDir, 'manifest.json'), JSON.stringify(manifestData));

    const code = "import { graphql } from '~gql'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, { outputDir }, path.join(pkgDir, 'src', 'components', 'Test.tsx'));

    expect(output).toContain('import { MyQueryDocument } from "../generated/query.codegen";');
  });

  it('rewrites graphql calls when outputDir is relative and cwd differs from package root', () => {
    const rootDir = tmpDir;
    const pkgDir = path.join(rootDir, 'packages', 'app');
    const outputDir = './src/generated';
    const absoluteOutputDir = path.join(pkgDir, 'src', 'generated');

    fs.mkdirSync(absoluteOutputDir, { recursive: true });
    fs.writeFileSync(
      path.join(pkgDir, 'package.json'),
      JSON.stringify({ name: 'app' }),
    );
    fs.writeFileSync(
      path.join(absoluteOutputDir, 'manifest.json'),
      JSON.stringify(manifestData),
    );

    const code =
      "import { graphql } from './generated/graphql'; const q = graphql(`query { me { id } }`);";
    const pluginPath = path.join(process.cwd(), 'index.js');
    const presetPath = require.resolve('@babel/preset-typescript');
    const filePath = path.join(pkgDir, 'src', 'test.ts');

    const output = execFileSync(
      process.execPath,
      [
        '-e',
        `
          const babel = require('@babel/core');
          const plugin = require(${JSON.stringify(pluginPath)});
          process.chdir(${JSON.stringify(rootDir)});
          const result = babel.transformSync(${JSON.stringify(code)}, {
            plugins: [[plugin, { outputDir: ${JSON.stringify(outputDir)} }]],
            presets: [${JSON.stringify(presetPath)}],
            filename: ${JSON.stringify(filePath)},
            babelrc: false,
            configFile: false,
          });
          process.stdout.write(result.code);
        `,
      ],
      {
        cwd: process.cwd(),
        encoding: 'utf8',
      },
    );

    expect(output).toContain(
      'import { MyQueryDocument } from "./generated/query.codegen";',
    );
  });

  it('rewrites graphql calls when manifestPath is relative and cwd differs from package root', () => {
    const rootDir = tmpDir;
    const pkgDir = path.join(rootDir, 'packages', 'app');
    const outputDir = './src/generated';
    const absoluteOutputDir = path.join(pkgDir, 'src', 'generated');
    const relativeManifestPath = './custom-manifest.json';
    const absoluteManifestPath = path.join(pkgDir, 'custom-manifest.json');

    fs.mkdirSync(absoluteOutputDir, { recursive: true });
    fs.writeFileSync(
      path.join(pkgDir, 'package.json'),
      JSON.stringify({ name: 'app' }),
    );
    fs.writeFileSync(
      absoluteManifestPath,
      JSON.stringify(manifestData),
    );

    const code =
      "import { graphql } from './generated/graphql'; const q = graphql(`query { me { id } }`);";
    const pluginPath = path.join(process.cwd(), 'index.js');
    const presetPath = require.resolve('@babel/preset-typescript');
    const filePath = path.join(pkgDir, 'src', 'test.ts');

    const output = execFileSync(
      process.execPath,
      [
        '-e',
        `
          const babel = require('@babel/core');
          const plugin = require(${JSON.stringify(pluginPath)});
          process.chdir(${JSON.stringify(rootDir)});
          const result = babel.transformSync(${JSON.stringify(code)}, {
            plugins: [[plugin, { 
              outputDir: ${JSON.stringify(outputDir)},
              manifestPath: ${JSON.stringify(relativeManifestPath)}
            }]],
            presets: [${JSON.stringify(presetPath)}],
            filename: ${JSON.stringify(filePath)},
            babelrc: false,
            configFile: false,
          });
          process.stdout.write(result.code);
        `,
      ],
      {
        cwd: process.cwd(),
        encoding: 'utf8',
      },
    );

    expect(output).toContain(
      'import { MyQueryDocument } from "./generated/query.codegen";',
    );
  });
  
  it('ignores tsconfig paths that do not point to output dir', () => {
    const outputDir = path.join(tmpDir, 'gen');
    fs.mkdirSync(outputDir, { recursive: true });
    
    const tsconfig = {
      compilerOptions: {
        baseUrl: '.',
        paths: {
          '@other': ['other/path']
        }
      }
    };
    fs.writeFileSync(path.join(tmpDir, 'tsconfig.json'), JSON.stringify(tsconfig));
    fs.writeFileSync(path.join(outputDir, 'manifest.json'), JSON.stringify(manifestData));

    const code = "import { graphql } from '@other'; const q = graphql(`query { me { id } }`);";
    const output = transform(code, { outputDir }, path.join(tmpDir, 'test.ts'));

    expect(output).toContain("import { graphql } from '@other'");
    expect(output).not.toContain('MyQueryDocument');
  });
});
