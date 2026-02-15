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
    './packages/app-reference/src/**/*.ts',
    './packages/app-reference/**/*.graphql',
  ],
  generates: {
    './packages/schema/src/generated/graphql.ts': {
      plugins: ['typescript'],
    },
    './packages/ui-lib/src/generated/': {
      preset: 'client',
      plugins: [],
      presetConfig: {
        fileName: 'graphql.ts',
        fragmentMasking: false,
      },
    },
    './packages/app/src/generated/': {
      preset: 'client',
      plugins: [],
      presetConfig: {
        fileName: 'graphql.ts',
        fragmentMasking: false,
      },
    },
    './packages/app-reference/src/generated/': {
      preset: 'client',
      plugins: [],
      presetConfig: {
        fileName: 'graphql.ts',
        fragmentMasking: false,
      },
    },
    './packages/app-masking/src/generated/': {
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
