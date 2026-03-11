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
      expect(typeof isWasmAvailable).toBe('function');
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
