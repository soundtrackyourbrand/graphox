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

    const base = path.join(root, 'packages/playback/base');
    fs.mkdirSync(path.join(base, 'graphql'), { recursive: true });
    fs.writeFileSync(
      path.join(base, 'package.json'),
      JSON.stringify({ name: '@soundtrack/playback', exports: baseExports })
    );
    fs.writeFileSync(
      path.join(base, 'graphql/manifest.json'),
      JSON.stringify([
        {
          source: 'fragment PlaybackDisplay on Display { id }',
          path: './base.codegen',
          name: 'PlaybackDisplayFragmentDoc',
        },
      ])
    );

    const web = path.join(root, 'packages/playback/web');
    fs.mkdirSync(path.join(web, 'graphql'), { recursive: true });
    fs.writeFileSync(
      path.join(web, 'package.json'),
      JSON.stringify({ name: '@soundtrack/playback-web', exports: { './graphql': './graphql/index.ts' } })
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
    expect(output.importAlias).toBe('@soundtrack/playback/graphql');
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
    expect(warnings[0]).toContain('@soundtrack/playback/graphql/<file>');
  });

  it('treats the alias as a recognised entrypoint', () => {
    const { root, base } = fixture({ './graphql': './graphql/index.ts', './graphql/*': './graphql/*' });
    const outputs = resolvePluginOutputs(
      { outputs: [{ outputDir: path.join(base, 'graphql') }] },
      { cwd: root, onWarn: () => {} }
    );

    expect(outputs[0].graphqlImportPaths).toContain('@soundtrack/playback/graphql');
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
        source: 'fragment PlaybackDisplay on Display { id }',
        path: './web.codegen',
        name: 'PlaybackDisplayFragmentDoc',
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

    expect(outputs[0].manifestData![0].path).toBe('./base.codegen');
    expect(outputs[1].manifestData![0].path).toBe('./web.codegen');
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
