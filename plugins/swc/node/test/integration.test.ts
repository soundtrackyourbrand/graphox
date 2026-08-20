/**
 * Tests for the SWC GraphQL Plugin WASM Wrapper
 * 
 * These tests verify that:
 * 1. The WASM wrapper exports the correct functions
 * 2. Configuration validation works
 * 3. WASM path resolution works (when WASM is built)
 */

import { describe, it, expect } from 'vitest';
import {
  createSWCPlugin,
  isWasmAvailable,
  loadManifest,
  resolvePluginOutputs,
  PluginConfig
} from '../src/index.js';
import * as fs from 'fs';
import * as path from 'path';
import * as os from 'os';

import { fileURLToPath } from 'url';

describe('SWC Plugin WASM Wrapper', () => {
  describe('createSWCPlugin', () => {
    it('exports createSWCPlugin function', () => {
      expect(createSWCPlugin).toBeDefined();
      expect(typeof createSWCPlugin).toBe('function');
    });

    it('requires outputDir in config', () => {
      expect(() => {
        createSWCPlugin({} as PluginConfig);
      }).toThrow('outputDir is required');
    });

    it('resolves outputDir to absolute path', () => {
      // Create a dummy WASM file to make this test pass
      const testDir = path.dirname(fileURLToPath(import.meta.url));
      const wasmDir = path.join(testDir, '..', 'wasm');
      if (!fs.existsSync(wasmDir)) {
        fs.mkdirSync(wasmDir, { recursive: true });
      }
      const dummyWasmPath = path.join(wasmDir, 'graphox_swc_plugin.wasm');
      fs.writeFileSync(dummyWasmPath, '');

      try {
        const result = createSWCPlugin({
          outputDir: './gen'
        });

        expect(path.isAbsolute(result[1].outputDir)).toBe(true);
        expect(result[1].outputDir).toBe(path.resolve('./gen'));
      } finally {
        fs.unlinkSync(dummyWasmPath);
      }
    });

    it('automatically uses manifest.json in outputDir if manifestPath is missing', () => {
      const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'swc-test-'));
      const outputDir = path.join(tempDir, 'gen');
      fs.mkdirSync(outputDir, { recursive: true });
      
      const manifestData = [
        { source: 'query { me { id } }', path: './gen/query.codegen', name: 'GetMeDocument' }
      ];
      fs.writeFileSync(path.join(outputDir, 'manifest.json'), JSON.stringify(manifestData));

      // Create dummy WASM to avoid getWasmPath error
      const testDir = path.dirname(fileURLToPath(import.meta.url));
      const wasmDir = path.join(testDir, '..', 'wasm');
      if (!fs.existsSync(wasmDir)) fs.mkdirSync(wasmDir, { recursive: true });
      const dummyWasmPath = path.join(wasmDir, 'graphox_swc_plugin.wasm');
      fs.writeFileSync(dummyWasmPath, '');

      try {
        const result = createSWCPlugin({
          outputDir: outputDir
        });
        
        expect(result[1].manifestData).toEqual(manifestData);
      } finally {
        fs.unlinkSync(dummyWasmPath);
        fs.rmSync(tempDir, { recursive: true });
      }
    });

    it('reads imports from package.json and tsconfig.json', () => {
      const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'swc-test-'));
      const outputDir = path.join(tempDir, 'gen');
      fs.mkdirSync(outputDir, { recursive: true });

      const tsconfig = {
        compilerOptions: {
          baseUrl: '.',
          paths: {
            '@gql': ['gen/graphql.ts']
          }
        }
      };
      fs.writeFileSync(path.join(tempDir, 'tsconfig.json'), JSON.stringify(tsconfig));

      const pkgJson = {
        imports: {
          '#gql': './gen/graphql.ts'
        }
      };
      fs.writeFileSync(path.join(tempDir, 'package.json'), JSON.stringify(pkgJson));

      // Create dummy WASM
      const testDir = path.dirname(fileURLToPath(import.meta.url));
      const wasmDir = path.join(testDir, '..', 'wasm');
      if (!fs.existsSync(wasmDir)) fs.mkdirSync(wasmDir, { recursive: true });
      const dummyWasmPath = path.join(wasmDir, 'graphox_swc_plugin.wasm');
      fs.writeFileSync(dummyWasmPath, '');

      try {
        const result = createSWCPlugin({
          outputDir: 'gen'
        }, { cwd: tempDir });

        expect(result[1].graphqlImportPaths).toContain('@gql');
        expect(result[1].graphqlImportPaths).toContain('#gql');
      } finally {
        fs.unlinkSync(dummyWasmPath);
        fs.rmSync(tempDir, { recursive: true });
      }
    });

    it('resolves outputDir relative to package root in monorepo when options.cwd is missing', () => {
      const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'swc-monorepo-test-auto-'));
      const pkgDir = path.join(tempDir, 'packages', 'app');
      const outputDir = './src/generated';
      const absoluteOutputDir = path.join(pkgDir, 'src', 'generated');

      fs.mkdirSync(absoluteOutputDir, { recursive: true });
      fs.writeFileSync(
        path.join(pkgDir, 'package.json'),
        JSON.stringify({ name: 'app' }),
      );

      const manifestData = [
        { source: 'query { me { id } }', path: './query.codegen', name: 'MyQueryDocument' }
      ];
      fs.writeFileSync(
        path.join(absoluteOutputDir, 'manifest.json'),
        JSON.stringify(manifestData),
      );

      // Create dummy WASM
      const testDir = path.dirname(fileURLToPath(import.meta.url));
      const wasmDir = path.join(testDir, '..', 'wasm');
      if (!fs.existsSync(wasmDir)) fs.mkdirSync(wasmDir, { recursive: true });
      const dummyWasmPath = path.join(wasmDir, 'graphox_swc_plugin.wasm');
      fs.writeFileSync(dummyWasmPath, '');

      try {
        // We use filename to trigger root detection
        const result = createSWCPlugin({
          outputDir: outputDir
        }, { 
          filename: path.join(pkgDir, 'src', 'test.ts')
        });

        expect(result[1].outputDir).toBe(absoluteOutputDir);
        expect(result[1].manifestData).toEqual(manifestData);
      } finally {
        fs.unlinkSync(dummyWasmPath);
        fs.rmSync(tempDir, { recursive: true });
      }
    });

    it('resolves outputDir relative to package root in monorepo when cwd differs', () => {
      const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'swc-monorepo-test-'));
      const pkgDir = path.join(tempDir, 'packages', 'app');
      const outputDir = './src/generated';
      const absoluteOutputDir = path.join(pkgDir, 'src', 'generated');

      fs.mkdirSync(absoluteOutputDir, { recursive: true });
      fs.writeFileSync(
        path.join(pkgDir, 'package.json'),
        JSON.stringify({ name: 'app' }),
      );

      const manifestData = [
        { source: 'query { me { id } }', path: './query.codegen', name: 'MyQueryDocument' }
      ];
      fs.writeFileSync(
        path.join(absoluteOutputDir, 'manifest.json'),
        JSON.stringify(manifestData),
      );

      // Create dummy WASM
      const testDir = path.dirname(fileURLToPath(import.meta.url));
      const wasmDir = path.join(testDir, '..', 'wasm');
      if (!fs.existsSync(wasmDir)) fs.mkdirSync(wasmDir, { recursive: true });
      const dummyWasmPath = path.join(wasmDir, 'graphox_swc_plugin.wasm');
      fs.writeFileSync(dummyWasmPath, '');

      try {
        // Run with cwd = tempDir (monorepo root), but options.cwd = pkgDir (package root)
        const result = createSWCPlugin({
          outputDir: outputDir
        }, { cwd: pkgDir });

        expect(result[1].outputDir).toBe(absoluteOutputDir);
        expect(result[1].manifestData).toEqual(manifestData);
      } finally {
        fs.unlinkSync(dummyWasmPath);
        fs.rmSync(tempDir, { recursive: true });
      }
    });

    it('does not leak graphqlImportPaths between calls through cache', () => {
      const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'swc-test-leak-'));
      const outputDir = path.join(tempDir, 'gen');
      fs.mkdirSync(outputDir, { recursive: true });

      // Create dummy WASM
      const testDir = path.dirname(fileURLToPath(import.meta.url));
      const wasmDir = path.join(testDir, '..', 'wasm');
      if (!fs.existsSync(wasmDir)) fs.mkdirSync(wasmDir, { recursive: true });
      const dummyWasmPath = path.join(wasmDir, 'graphox_swc_plugin.wasm');
      fs.writeFileSync(dummyWasmPath, '');

      try {
        // First call with one path
        const result1 = createSWCPlugin({
          outputDir: 'gen',
          graphqlImportPaths: ['@first']
        }, { cwd: tempDir });

        expect(result1[1].graphqlImportPaths).toContain('@first');
        expect(result1[1].graphqlImportPaths).not.toContain('@second');

        // Second call with another path
        const result2 = createSWCPlugin({
          outputDir: 'gen',
          graphqlImportPaths: ['@second']
        }, { cwd: tempDir });

        expect(result2[1].graphqlImportPaths).toContain('@second');
        // This is the crucial check: @first should NOT be here if it was correctly NOT cached
        expect(result2[1].graphqlImportPaths).not.toContain('@first');
      } finally {
        fs.unlinkSync(dummyWasmPath);
        fs.rmSync(tempDir, { recursive: true });
      }
    });

    it('returns [wasmPath, config] tuple when WASM exists', () => {
      // This test will fail until WASM is built
      // That's expected behavior
      try {
        const result = createSWCPlugin({
          outputDir: './gen'
        });

        expect(Array.isArray(result)).toBe(true);
        expect(result.length).toBe(2);
        expect(typeof result[0]).toBe('string'); // WASM path
        expect(typeof result[1]).toBe('object'); // Config
      } catch (e) {
        // Expected if WASM not built
        expect((e as Error).message).toContain('WASM plugin not found');
      }
    });
  });

  describe('isWasmAvailable', () => {
    it('exports isWasmAvailable function', () => {
      expect(isWasmAvailable).toBeDefined();
      expect(typeof isWasmAvailable()).toBe('boolean');
    });

    it('returns false when WASM is not built', () => {
      // Before building WASM, this should return false
      const available = isWasmAvailable();
      expect(typeof available).toBe('boolean');
    });
  });

  describe('loadManifest', () => {
    it('exports loadManifest function', () => {
      expect(loadManifest).toBeDefined();
      expect(typeof loadManifest).toBe('function');
    });

    it('returns empty array when no manifest data provided', () => {
      const result = loadManifest({
        outputDir: './gen'
      });
      
      expect(Array.isArray(result)).toBe(true);
      expect(result.length).toBe(0);
    });

    it('returns manifestData when provided', () => {
      const manifestData = [
        { source: 'query { me { id } }', path: './gen/query.codegen', name: 'GetMeDocument' }
      ];
      
      const result = loadManifest({
        outputDir: './gen',
        manifestData
      });
      
      expect(result).toEqual(manifestData);
    });

    it('loads manifest from file when manifestPath provided', () => {
      // Create a temporary manifest file
      const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'swc-test-'));
      const manifestPath = path.join(tempDir, 'manifest.json');
      const manifestData = {
        entries: [
          { source: 'query { me { id } }', path: './gen/query.codegen', name: 'GetMeDocument' }
        ]
      };
      
      fs.writeFileSync(manifestPath, JSON.stringify(manifestData));
      
      try {
        const result = loadManifest({
          outputDir: './gen',
          manifestPath
        });
        
        expect(result).toEqual(manifestData.entries);
      } finally {
        fs.rmSync(tempDir, { recursive: true });
      }
    });

    it('manifestData takes precedence over manifestPath', () => {
      const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'swc-test-'));
      const manifestPath = path.join(tempDir, 'manifest.json');
      const fileData = {
        entries: [
          { source: 'query { me { id } }', path: './gen/file.codegen', name: 'FileDocument' }
        ]
      };
      
      fs.writeFileSync(manifestPath, JSON.stringify(fileData));
      
      const inlineData = [
        { source: 'query { me { id } }', path: './gen/inline.codegen', name: 'InlineDocument' }
      ];
      
      try {
        const result = loadManifest({
          outputDir: './gen',
          manifestPath,
          manifestData: inlineData
        });
        
        expect(result).toEqual(inlineData);
      } finally {
        fs.rmSync(tempDir, { recursive: true });
      }
    });
  });
});

describe('multi-project outputs', () => {
  /**
   * Two packages in one workspace. `base` owns a fragment and exposes its
   * generated directory; `web` consumes it. Written to disk because the alias
   * and package root are inferred from real package.json files.
   */
  function fixture(baseExports: Record<string, unknown>) {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'graphox-multi-'));

    const base = path.join(root, 'packages/catalog');
    fs.mkdirSync(path.join(base, 'graphql'), { recursive: true });
    fs.writeFileSync(
      path.join(base, 'package.json'),
      JSON.stringify({ name: '@example/catalog', exports: baseExports })
    );
    fs.writeFileSync(
      path.join(base, 'graphql/manifest.json'),
      JSON.stringify([
        {
          source: 'fragment ProductCard on Product { id }',
          path: './catalog.codegen',
          name: 'ProductCardFragmentDoc',
        },
      ])
    );

    const web = path.join(root, 'packages/storefront');
    fs.mkdirSync(path.join(web, 'graphql'), { recursive: true });
    fs.writeFileSync(
      path.join(web, 'package.json'),
      JSON.stringify({ name: '@example/catalog-web', exports: { './graphql': './graphql/index.ts' } })
    );
    fs.writeFileSync(path.join(web, 'graphql/manifest.json'), JSON.stringify([]));

    return { root, base, web };
  }

  it('infers importAlias and packageRoot from the owning package', () => {
    const { root, base } = fixture({
      './graphql': './graphql/index.ts',
      './graphql/*': './graphql/*',
    });
    const warnings: string[] = [];

    const outputs = resolvePluginOutputs(
      { outputs: [{ outputDir: path.join(base, 'graphql') }] },
      { cwd: root, onWarn: (m) => warnings.push(m) }
    );

    const output = outputs[0];
    expect(output.importAlias).toBe('@example/catalog/graphql');
    expect(output.packageRoot).toBe(base);
    expect(warnings).toEqual([]);
  });

  it('warns when the exports map cannot serve files inside the subpath', () => {
    const { root, base } = fixture({ './graphql': './graphql/index.ts' });
    const warnings: string[] = [];

    resolvePluginOutputs(
      { outputs: [{ outputDir: path.join(base, 'graphql') }] },
      { cwd: root, onWarn: (m) => warnings.push(m) }
    );

    expect(warnings).toHaveLength(1);
    expect(warnings[0]).toContain('"./graphql/*": "./graphql/*"');
    expect(warnings[0]).toContain('@example/catalog/graphql/<file>');
  });

  it('treats the alias as a recognised entrypoint', () => {
    const { root, base } = fixture({ './graphql': './graphql/index.ts', './graphql/*': './graphql/*' });
    const outputs = resolvePluginOutputs(
      { outputs: [{ outputDir: path.join(base, 'graphql') }] },
      { cwd: root, onWarn: () => {} }
    );

    expect(outputs[0].graphqlImportPaths).toContain('@example/catalog/graphql');
  });

  it('inlines a manifest per output', () => {
    const { root, base, web } = fixture({ './graphql': './graphql/index.ts', './graphql/*': './graphql/*' });
    const outputs = resolvePluginOutputs(
      {
        outputs: [
          { outputDir: path.join(base, 'graphql') },
          { outputDir: path.join(web, 'graphql') },
        ],
      },
      { cwd: root, onWarn: () => {} }
    );

    expect(outputs).toHaveLength(2);
    expect(outputs[0].manifestData).toHaveLength(1);
    expect(outputs[1].manifestData).toHaveLength(0);
  });

  it('rejects a duplicate outputDir', () => {
    const { root, base } = fixture({ './graphql': './graphql/index.ts' });
    const dir = path.join(base, 'graphql');
    expect(() =>
      resolvePluginOutputs({ outputs: [{ outputDir: dir }, { outputDir: dir }] }, { cwd: root, onWarn: () => {} })
    ).toThrow(/duplicate outputDir/);
  });

  it('rejects nested outputDirs', () => {
    const { root, base } = fixture({ './graphql': './graphql/index.ts' });
    expect(() =>
      resolvePluginOutputs(
        {
          outputs: [
            { outputDir: path.join(base, 'graphql') },
            { outputDir: path.join(base, 'graphql/nested') },
          ],
        },
        { cwd: root, onWarn: () => {} }
      )
    ).toThrow(/overlap/);
  });

  it('allows the same document name and source in two outputs', () => {
    // Two projects sharing a document name is normal, and the plugin resolves
    // per entrypoint rather than across a merged map, so it is not ambiguous.
    const { root, base, web } = fixture({ './graphql': './graphql/index.ts' });
    const shared = [
      {
        source: 'fragment ProductCard on Product { id }',
        path: './storefront.codegen',
        name: 'ProductCardFragmentDoc',
      },
    ];

    const outputs = resolvePluginOutputs(
      {
        outputs: [
          { outputDir: path.join(base, 'graphql') },
          { outputDir: path.join(web, 'graphql'), manifestData: shared },
        ],
      },
      { cwd: root, onWarn: () => {} }
    );

    expect(outputs[0].manifestData![0].path).toBe('./catalog.codegen');
    expect(outputs[1].manifestData![0].path).toBe('./storefront.codegen');
  });

  it('still accepts the single-output form', () => {
    const { root, base } = fixture({ './graphql': './graphql/index.ts' });
    const outputs = resolvePluginOutputs(
      { outputDir: path.join(base, 'graphql') },
      { cwd: root, onWarn: () => {} }
    );

    expect(outputs).toHaveLength(1);
    expect(outputs[0].manifestData).toHaveLength(1);
    expect(outputs[0].outputDir).toBe(path.join(base, 'graphql'));
  });
});

/**
 * How a project's own call sites name its entrypoint. This matters more than it
 * looks: the plugin empties an entrypoint whenever the file path matches, but it
 * only rewrites call sites whose import specifier it recognises. An alias shape
 * we cannot read means the entrypoint is emptied while its callers are left
 * calling it — valid JavaScript that fails when the document reaches the client.
 */
describe('entrypoint alias detection', () => {
  function workspace(files: Record<string, unknown>, outputSubdir: string) {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'graphox-alias-'));
    for (const [relative, contents] of Object.entries(files)) {
      const target = path.join(root, relative);
      fs.mkdirSync(path.dirname(target), { recursive: true });
      fs.writeFileSync(target, JSON.stringify(contents));
    }
    fs.mkdirSync(path.join(root, outputSubdir), { recursive: true });
    fs.writeFileSync(path.join(root, outputSubdir, 'manifest.json'), JSON.stringify([]));
    return root;
  }

  it('resolves a package.json imports wildcard that has a suffix after the star', () => {
    // "#lucy-graphql/*" -> "./lucy-graphql/*.ts": call sites write
    // `import { graphql } from '#lucy-graphql/playback/graphql'`, so that exact
    // specifier is what has to be recognised.
    const root = workspace(
      {
        'packages/web/package.json': {
          name: '@example/web',
          imports: { '#lucy-graphql/*': './lucy-graphql/*.ts' },
        },
      },
      'packages/web/lucy-graphql/playback'
    );

    try {
      const outputs = resolvePluginOutputs(
        { outputs: [{ outputDir: 'packages/web/lucy-graphql/playback' }] },
        { cwd: root }
      );

      expect(outputs[0].graphqlImportPaths).toContain('#lucy-graphql/playback/graphql');
      expect(outputs[0].graphqlImportPaths).toContain('#lucy-graphql/playback/index');
    } finally {
      fs.rmSync(root, { recursive: true });
    }
  });

  it('resolves a tsconfig paths wildcard that has a suffix after the star', () => {
    const root = workspace(
      {
        'tsconfig.json': {
          compilerOptions: { baseUrl: '.', paths: { '@gen/*': ['gen/*.ts'] } },
        },
      },
      'gen/playback'
    );

    try {
      const outputs = resolvePluginOutputs({ outputs: [{ outputDir: 'gen/playback' }] }, { cwd: root });
      expect(outputs[0].graphqlImportPaths).toContain('@gen/playback/graphql');
    } finally {
      fs.rmSync(root, { recursive: true });
    }
  });

  it('resolves a wildcard whose target names the entrypoint file directly', () => {
    // "@gen/*" -> "gen/*/graphql.ts": the star stands for the project directory,
    // so the specifier is the alias with no trailing file segment.
    const root = workspace(
      {
        'tsconfig.json': {
          compilerOptions: { baseUrl: '.', paths: { '@gen/*': ['gen/*/graphql.ts'] } },
        },
      },
      'gen/playback'
    );

    try {
      const outputs = resolvePluginOutputs({ outputs: [{ outputDir: 'gen/playback' }] }, { cwd: root });
      expect(outputs[0].graphqlImportPaths).toContain('@gen/playback');
    } finally {
      fs.rmSync(root, { recursive: true });
    }
  });

  it('reads a wildcard target behind a condition key', () => {
    const root = workspace(
      {
        'packages/web/package.json': {
          name: '@example/web',
          imports: { '#gen/*': { node: './gen/*.ts', default: './gen/*.js' } },
        },
      },
      'packages/web/gen/playback'
    );

    try {
      const outputs = resolvePluginOutputs(
        { outputs: [{ outputDir: 'packages/web/gen/playback' }] },
        { cwd: root }
      );
      expect(outputs[0].graphqlImportPaths).toContain('#gen/playback/graphql');
    } finally {
      fs.rmSync(root, { recursive: true });
    }
  });

  it('leaves an unrelated alias alone', () => {
    const root = workspace(
      {
        'packages/web/package.json': {
          name: '@example/web',
          imports: { '#utils/*': './src/utils/*.ts' },
        },
      },
      'packages/web/gen/playback'
    );

    try {
      const warnings: string[] = [];
      const outputs = resolvePluginOutputs(
        { outputs: [{ outputDir: 'packages/web/gen/playback' }] },
        { cwd: root, onWarn: (message) => warnings.push(message) }
      );

      expect(outputs[0].graphqlImportPaths).not.toContain('#utils/');
      expect(warnings).toHaveLength(0);
    } finally {
      fs.rmSync(root, { recursive: true });
    }
  });

  it('warns when an alias leads into the output but yields no specifier', () => {
    // The star reaches the output directory, but nothing it can stand for lands
    // on the entrypoint. Silence here is what took down a production build.
    const root = workspace(
      {
        'packages/web/package.json': {
          name: '@example/web',
          imports: { '#gen/*': './gen/*/documents.ts' },
        },
      },
      'packages/web/gen/playback'
    );

    try {
      const warnings: string[] = [];
      resolvePluginOutputs(
        { outputs: [{ outputDir: 'packages/web/gen/playback' }] },
        { cwd: root, onWarn: (message) => warnings.push(message) }
      );

      expect(warnings).toHaveLength(1);
      expect(warnings[0]).toContain('#gen/*');
      expect(warnings[0]).toContain('graphqlImportPaths');
    } finally {
      fs.rmSync(root, { recursive: true });
    }
  });
});

describe('output directory ambiguity', () => {
  it('warns when a sibling file shadows the output directory specifier', () => {
    // `./gen` resolves to gen.ts, not gen/ — a file beats a directory. The plugin
    // has no filesystem and reads the directory form as the generated barrel.
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'graphox-shadow-'));
    fs.mkdirSync(path.join(root, 'gen'), { recursive: true });
    fs.writeFileSync(path.join(root, 'gen/manifest.json'), JSON.stringify([]));
    fs.writeFileSync(path.join(root, 'gen.ts'), 'export const handWritten = 1;');

    try {
      const warnings: string[] = [];
      resolvePluginOutputs(
        { outputs: [{ outputDir: 'gen' }] },
        { cwd: root, onWarn: (message) => warnings.push(message) }
      );

      expect(warnings).toHaveLength(1);
      expect(warnings[0]).toContain('gen.ts');
      expect(warnings[0]).toContain('gen/graphql');
    } finally {
      fs.rmSync(root, { recursive: true });
    }
  });

  it('stays quiet when nothing shadows it', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'graphox-noshadow-'));
    fs.mkdirSync(path.join(root, 'gen'), { recursive: true });
    fs.writeFileSync(path.join(root, 'gen/manifest.json'), JSON.stringify([]));

    try {
      const warnings: string[] = [];
      resolvePluginOutputs(
        { outputs: [{ outputDir: 'gen' }] },
        { cwd: root, onWarn: (message) => warnings.push(message) }
      );

      expect(warnings).toHaveLength(0);
    } finally {
      fs.rmSync(root, { recursive: true });
    }
  });
});

describe('alias scan caching', () => {
  it('picks up an alias added after a first resolve', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'graphox-cache-'));
    fs.mkdirSync(path.join(root, 'gen'), { recursive: true });
    fs.writeFileSync(path.join(root, 'gen/manifest.json'), JSON.stringify([]));
    const pkgJson = path.join(root, 'package.json');
    fs.writeFileSync(pkgJson, JSON.stringify({ name: 'x' }));

    try {
      const before = resolvePluginOutputs({ outputs: [{ outputDir: 'gen' }] }, { cwd: root });
      expect(before[0].graphqlImportPaths).not.toContain('#gen/graphql');

      fs.writeFileSync(
        pkgJson,
        JSON.stringify({ name: 'x', imports: { '#gen/*': './gen/*.ts' } })
      );
      // mtime resolution can be coarse; make the change unambiguous.
      const future = new Date(Date.now() + 2000);
      fs.utimesSync(pkgJson, future, future);

      const after = resolvePluginOutputs({ outputs: [{ outputDir: 'gen' }] }, { cwd: root });
      expect(after[0].graphqlImportPaths).toContain('#gen/graphql');
    } finally {
      fs.rmSync(root, { recursive: true });
    }
  });
});
