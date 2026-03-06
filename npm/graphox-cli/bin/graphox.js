#!/usr/bin/env node

"use strict";

const { spawnSync } = require('child_process');
const path = require('path');
const fs = require('fs');

const binaryName = process.platform === 'win32' ? 'graphox-bin.exe' : 'graphox-bin';
const binaryPath = path.join(__dirname, binaryName);

if (!fs.existsSync(binaryPath)) {
  console.error(`Error: Graphox native binary not found at ${binaryPath}`);
  console.error("This usually means the postinstall script was skipped (for pnpm, check ignored builds).");
  console.error("Try: pnpm approve-builds @graphox/cli && pnpm rebuild @graphox/cli");
  process.exit(1);
}

const result = spawnSync(binaryPath, process.argv.slice(2), {
  stdio: 'inherit',
  shell: false
});

if (result.error) {
  if (result.error.code === 'ENOENT') {
    console.error(`Error: Could not execute binary at ${binaryPath}. It might be missing or not executable.`);
  } else {
    console.error('Failed to start Graphox CLI:', result.error.message);
  }
  process.exit(1);
}

process.exit(result.status === null ? 1 : result.status);
