const esbuild = require('esbuild');

const watch = process.argv.includes('--watch');

const buildOptions = {
  entryPoints: ['src/extension.ts'],
  bundle: true,
  format: 'cjs',
  platform: 'node',
  target: 'node16',
  outfile: 'out/extension.js',
  external: ['vscode'],
  sourcemap: false,
  logLevel: 'info'
};

async function run() {
  if (watch) {
    const ctx = await esbuild.context(buildOptions);
    await ctx.watch();
    return;
  }

  await esbuild.build(buildOptions);
}

run().catch((error) => {
  console.error(error);
  process.exit(1);
});
