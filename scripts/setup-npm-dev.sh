#!/bin/bash
set -e

# Script to set up the NPM package for local development
# This builds the Rust binary and installs it in the npm package

echo "Building Rust binary..."
cargo build --release

BINARY_PATH="$(pwd)/target/release/graphox"
NPM_PACKAGE_DIR="$(pwd)/npm/graphox-cli"

if [ ! -f "$BINARY_PATH" ]; then
    echo "Error: Binary not found at $BINARY_PATH"
    echo "Make sure you run this script from the project root after building with 'cargo build --release'."
    exit 1
fi

echo "Setting up npm package with local build via symlink..."
export GRAPHOX_LOCAL_BUILD="$BINARY_PATH"

cd "$NPM_PACKAGE_DIR"

# Ensure the bin directory exists
mkdir -p bin

# Run install script which will create the symlink
node postinstall.js

echo ""
echo "✓ Local development setup complete!"
echo ""
echo "The npm package now uses a symlink to your local binary:"
echo "  $BINARY_PATH"
echo ""
echo "After making changes to the Rust code, simply run:"
echo "  cargo build --release"
echo "The changes will be automatically picked up by the linked npm package."
echo ""
echo "To use this package in another project:"
echo "  cd /path/to/your/project"
echo "  pnpm link $NPM_PACKAGE_DIR"
echo ""
echo "Alternatively, you can try global linking (may require 'pnpm setup'):"
echo "  cd $NPM_PACKAGE_DIR"
echo "  pnpm link --global"
echo "  # Then in your project:"
echo "  pnpm link --global @graphox/cli"
