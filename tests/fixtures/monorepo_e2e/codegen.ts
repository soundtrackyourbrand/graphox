import type { CodegenConfig } from '@graphql-codegen/cli';

const config: CodegenConfig = {
  schema: './packages/schema/schema.graphql',
  documents: [
    './packages/ui-lib/src/**/*.ts',
    './packages/ui-lib/**/*.graphql',
    './packages/app/src/**/*.ts',
    './packages/app/**/*.graphql',
    './packages/app-masking/src/**/*.ts',
    './packages/app-masking/**/*.graphql',
  ],
  generates: {
    './packages/schema/src/__generated__/graphql.ts': {
      plugins: ['typescript'],
    },
    './packages/ui-lib/src/__generated__/': {
      preset: 'client',
      plugins: [],
      presetConfig: {
        fileName: 'graphql.ts',
        fragmentMasking: false,
      },
    },
    './packages/app/src/__generated__/': {
      preset: 'client',
      plugins: [],
      presetConfig: {
        fileName: 'graphql.ts',
        fragmentMasking: false,
      },
    },
    './packages/app-masking/src/__generated__/': {
      preset: 'client',
      plugins: [],
      presetConfig: {
        fileName: 'graphql.ts',
        fragmentMasking: {
          unmaskFunctionName: 'getFragmentData',
        },
      },
    },
  },
};

export default config;
