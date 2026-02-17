#!/usr/bin/env node

const fs = require('fs');
const path = require('path');

const PLATFORMS = [
  'darwin-arm64',
  'darwin-x64',
  'linux-arm64',
  'linux-x64',
  'win32-arm64',
  'win32-x64'
];

function getPlatform() {
  const platform = process.platform;
  const arch = process.arch;
  return `${platform}-${arch}`;
}

function install() {
  const currentPlatform = getPlatform();
  const binDir = path.join(__dirname, 'bin');
  const binaryName = process.platform === 'win32' ? 'graphox-bin.exe' : 'graphox-bin';
  const binaryPath = path.join(binDir, binaryName);

  if (!fs.existsSync(binDir)) {
    fs.mkdirSync(binDir, { recursive: true });
  }

  // 1. Check for local build first (for development)
  if (process.env.GRAPHOX_LOCAL_BUILD) {
    try {
      fs.copyFileSync(process.env.GRAPHOX_LOCAL_BUILD, binaryPath);
      fs.chmodSync(binaryPath, 0o755);
      console.log('Using local build.');
      return;
    } catch (e) {
      console.error('Failed to use local build:', e.message);
    }
  }

  // 2. Try to find the binary from optionalDependencies
  const pkgName = `@soundtrackyourbrand/graphox-${currentPlatform}`;
  try {
    const pkgPath = require.resolve(`${pkgName}/bin/graphox${process.platform === 'win32' ? '.exe' : ''}`);
    
    // Copy or link the binary
    fs.copyFileSync(pkgPath, binaryPath);
    fs.chmodSync(binaryPath, 0o755);
    console.log(`Successfully installed binary from ${pkgName}`);
  } catch (e) {
    console.error(`Error: Could not find platform-specific package ${pkgName}.`);
    console.error('This typically means the package was not installed as an optional dependency.');
    console.error('Please ensure you have access to @soundtrackyourbrand scope on GitHub Packages.');
    process.exit(1);
  }
}

install();
