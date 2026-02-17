import { defineConfig } from '@rsbuild/core';
import { pluginBabel } from '@rsbuild/plugin-babel';
import { createSWCPlugin } from '@soundtrackyourbrand/graphox-swc';
import graphoxBabel from '@soundtrackyourbrand/graphox-babel';
import * as path from 'path';

const mode = process.env.RSBUILD_MODE || 'swc';
const appGeneratedDir = path.resolve(__dirname, '../app/src/__generated__');

const swcPlugin = mode === 'swc' ? createSWCPlugin({
  manifestPath: path.join(appGeneratedDir, 'manifest.json'),
  outputDir: appGeneratedDir,
}) : null;

export default defineConfig({
  plugins: mode === 'babel' ? [
    pluginBabel({
      include: [/\.(ts|tsx)$/],
      babelLoaderOptions: {
        plugins: [
          [
            graphoxBabel,
            {
              manifestPath: path.join(appGeneratedDir, 'manifest.json'),
              outputDir: appGeneratedDir,
            },
          ],
        ],
      },
    }),
  ] : [],
  tools: {
    swc: (mode === 'swc' && swcPlugin) ? {
      jsc: {
        experimental: {
          plugins: [swcPlugin],
        },
      },
    } : {},
  },
  source: {
    entry: {
      index: './src/index.ts',
    },
    include: [
      path.resolve(__dirname, '../app/src'),
    ],
  },
  output: {
    distPath: {
      root: `dist-${mode}`,
    },
  },
});
