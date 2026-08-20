import { defineConfig } from '@rsbuild/core';
import { pluginBabel } from '@rsbuild/plugin-babel';
import { createSWCPlugin } from '@graphox/swc-plugin';
import graphoxBabel from '@graphox/babel-plugin';
import * as path from 'path';

const mode = process.env.RSBUILD_MODE || 'swc';
const appGeneratedDir = path.resolve(__dirname, '../app/src/__generated__');

// `@app/gql` is an rspack resolve.alias (below). Neither plugin can discover a
// bundler alias — they read package.json and tsconfig — so it has to be declared
// here, or call sites importing through it are left unrewritten while the
// entrypoint they call is emptied anyway.
const graphqlImportPaths = ['@app/gql'];

// This project spans two packages — graphox.yaml generates documents authored in
// rsbuild-app into the app package's output — so a call site here is outside the
// output's package and its rewritten import goes through the alias, not a
// relative path.
const importAlias = '@monorepo-e2e/app/src/__generated__';

const swcPlugin = mode === 'swc' ? createSWCPlugin({
  outputDir: appGeneratedDir,
  graphqlImportPaths,
  importAlias,
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
              outputDir: appGeneratedDir,
              graphqlImportPaths,
              importAlias,
            },
          ],
        ],
      },
    }),
  ] : [],
  tools: {
    rspack: {
      resolve: {
        alias: {
          '@app/gql': path.join(appGeneratedDir, 'graphql.ts'),
        },
      },
    },
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
