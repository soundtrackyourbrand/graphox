#!/usr/bin/env node

const { existsSync, mkdirSync, chmodSync, createWriteStream } = require('fs');
const { join } = require('path');
const { get: httpsGet } = require('https');
const { pipeline } = require('stream');
const { promisify } = require('util');

const streamPipeline = promisify(pipeline);

// Get platform and architecture
const PLATFORM_MAPPING = {
  darwin: 'apple-darwin',
  linux: 'unknown-linux-gnu',
  win32: 'pc-windows-msvc'
};

const ARCH_MAPPING = {
  x64: 'x86_64',
  arm64: 'aarch64'
};

function getPlatform() {
  const platform = process.platform;
  const arch = process.arch;

  const platformSuffix = PLATFORM_MAPPING[platform];
  const archPrefix = ARCH_MAPPING[arch];

  if (!platformSuffix || !archPrefix) {
    throw new Error(
      `Unsupported platform: ${platform}-${arch}. ` +
      `Supported platforms: darwin-x64, darwin-arm64, linux-x64, linux-arm64, win32-x64, win32-arm64`
    );
  }

  return `${archPrefix}-${platformSuffix}`;
}

function getBinaryName() {
  return process.platform === 'win32' ? 'graphql-rust.exe' : 'graphql-rust';
}

function getDownloadURL(version) {
  const target = getPlatform();
  const ext = process.platform === 'win32' ? 'zip' : 'tar.gz';
  const baseURL = process.env.GRAPHQL_RUST_DOWNLOAD_URL || 
    'https://github.com/YOUR_USERNAME/graphql-rust/releases/download';
  
  return `${baseURL}/v${version}/graphql-rust-${target}.${ext}`;
}

async function downloadFile(url, destPath) {
  return new Promise((resolve, reject) => {
    console.log(`Downloading from ${url}...`);
    
    httpsGet(url, (response) => {
      // Follow redirects
      if (response.statusCode === 302 || response.statusCode === 301) {
        downloadFile(response.headers.location, destPath)
          .then(resolve)
          .catch(reject);
        return;
      }

      if (response.statusCode !== 200) {
        reject(new Error(`Download failed with status ${response.statusCode}`));
        return;
      }

      const fileStream = createWriteStream(destPath);
      streamPipeline(response, fileStream)
        .then(resolve)
        .catch(reject);
    }).on('error', reject);
  });
}

async function extractArchive(archivePath, destDir) {
  const { execSync } = require('child_process');
  
  if (process.platform === 'win32') {
    // Use PowerShell on Windows
    execSync(`powershell -command "Expand-Archive -Path '${archivePath}' -DestinationPath '${destDir}' -Force"`, {
      stdio: 'inherit'
    });
  } else {
    // Use tar on Unix-like systems
    execSync(`tar -xzf "${archivePath}" -C "${destDir}"`, {
      stdio: 'inherit'
    });
  }
}

async function install() {
  try {
    const packageJson = require('./package.json');
    const version = packageJson.version;
    const binDir = join(__dirname, 'bin');
    const binaryName = getBinaryName();
    const binaryPath = join(binDir, binaryName);

    // Check if binary already exists
    if (existsSync(binaryPath)) {
      console.log('Binary already installed.');
      return;
    }

    // Check for local development build
    // If GRAPHQL_RUST_LOCAL_BUILD is set, try to copy from local build
    const localBuildPath = process.env.GRAPHQL_RUST_LOCAL_BUILD;
    if (localBuildPath) {
      console.log(`Using local build from: ${localBuildPath}`);
      try {
        const { copyFileSync } = require('fs');
        
        // Create bin directory
        if (!existsSync(binDir)) {
          mkdirSync(binDir, { recursive: true });
        }

        // Copy the local binary
        copyFileSync(localBuildPath, binaryPath);
        
        // Make binary executable on Unix-like systems
        if (process.platform !== 'win32') {
          chmodSync(binaryPath, 0o755);
        }

        console.log('Local build installed successfully!');
        console.log(`Binary installed at: ${binaryPath}`);
        return;
      } catch (err) {
        console.error('Failed to use local build:', err.message);
        console.log('Falling back to downloading from releases...');
      }
    }

    // Create bin directory
    if (!existsSync(binDir)) {
      mkdirSync(binDir, { recursive: true });
    }

    // Download binary
    const downloadURL = getDownloadURL(version);
    const ext = process.platform === 'win32' ? 'zip' : 'tar.gz';
    const archivePath = join(binDir, `graphql-rust.${ext}`);

    await downloadFile(downloadURL, archivePath);
    console.log('Download complete.');

    // Extract archive
    console.log('Extracting binary...');
    await extractArchive(archivePath, binDir);

    // Make binary executable on Unix-like systems
    if (process.platform !== 'win32') {
      chmodSync(binaryPath, 0o755);
    }

    // Clean up archive
    const { unlinkSync } = require('fs');
    unlinkSync(archivePath);

    console.log('Installation complete!');
    console.log(`Binary installed at: ${binaryPath}`);
  } catch (error) {
    console.error('Installation failed:', error.message);
    console.error('\nYou can manually download the binary from:');
    console.error('https://github.com/YOUR_USERNAME/graphql-rust/releases');
    process.exit(1);
  }
}

install();
