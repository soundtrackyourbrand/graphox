const esbuild = require('esbuild');
const fs = require('fs');
const path = require('path');

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

async function copySchema() {
  const source = path.resolve(__dirname, '../../npm/graphox-cli/graphox.schema.json');
  const destination = path.resolve(__dirname, 'out/graphox.schema.json');
  
  if (!fs.existsSync(path.dirname(destination))) {
    fs.mkdirSync(path.dirname(destination), { recursive: true });
  }
  
  fs.copyFileSync(source, destination);
}

async function run() {
  await copySchema();

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
