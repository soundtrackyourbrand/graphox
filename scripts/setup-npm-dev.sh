#!/bin/bash
set -e

# Script to set up the NPM package for local development
# This builds the Rust binary and installs it in the npm package

echo "Building Rust binary..."
cargo build --release

BINARY_PATH="$(pwd)/target/release/graphql-rust"
NPM_PACKAGE_DIR="$(pwd)/npm/graphql-rust-cli"

if [ ! -f "$BINARY_PATH" ]; then
    echo "Error: Binary not found at $BINARY_PATH"
    echo "Make sure you run this script from the project root."
    exit 1
fi

echo "Setting up npm package with local build..."
export GRAPHQL_RUST_LOCAL_BUILD="$BINARY_PATH"

cd "$NPM_PACKAGE_DIR"

# Remove existing bin directory
rm -rf bin

# Run install script which will use the local build
node install.js

echo ""
echo "✓ Local development setup complete!"
echo ""
echo "The npm package now uses your local binary from:"
echo "  $BINARY_PATH"
echo ""
echo "To link globally for testing:"
echo "  cd npm/graphql-rust-cli"
echo "  pnpm link --global"
echo ""
echo "Then in any project:"
echo "  pnpm link --global graphql-rust-cli"
echo ""
echo "After making changes to the Rust code:"
echo "  cargo build --release"
echo "  ./scripts/setup-npm-dev.sh  # Run this script again"
