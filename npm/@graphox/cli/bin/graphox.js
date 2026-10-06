#!/usr/bin/env node
'use strict'

const PLATFORMS = {
  darwin: {
    arm64: '@graphox/darwin-arm64/graphox',
    x64: '@graphox/darwin-x64/graphox',
  },
  linux: {
    arm64: '@graphox/linux-arm64/graphox',
    x64: '@graphox/linux-x64/graphox',
  },
  win32: {
    arm64: '@graphox/win32-arm64/graphox.exe',
    x64: '@graphox/win32-x64/graphox.exe',
  },
}

const binPath = PLATFORMS[process.platform]?.[process.arch]

if (!binPath) {
  console.error(`Unsupported platform: ${process.platform} ${process.arch}`)
  process.exit(1)
}

const bin = require.resolve(binPath)
const result = require('child_process').spawnSync(bin, process.argv.slice(2), { stdio: 'inherit' })
if (result.error) {
  throw result.error
}
process.exitCode = result.status ?? 1
