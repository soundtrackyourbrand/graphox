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
      if (fs.existsSync(binaryPath)) {
        fs.unlinkSync(binaryPath);
      }
      
      const absoluteLocalBuild = path.resolve(process.env.GRAPHOX_LOCAL_BUILD);
      
      // Use symlink for local development so rebuilding Rust updates the CLI immediately
      fs.symlinkSync(absoluteLocalBuild, binaryPath);
      console.log(`Using local build via symlink: ${absoluteLocalBuild}`);
      return;
    } catch (e) {
      console.warn('Failed to create symlink for local build:', e.message);
      // Fallback to copy if symlink fails
      try {
        fs.copyFileSync(process.env.GRAPHOX_LOCAL_BUILD, binaryPath);
        fs.chmodSync(binaryPath, 0o755);
        console.log('Using local build (copy fallback).');
        return;
      } catch (copyErr) {
        console.error('Failed to copy local build:', copyErr.message);
      }
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
