#!/bin/bash
set -e

# Script to set up the SWC plugin for local development
# This builds the Rust WASM plugin and exports the environment variable

echo "Building Rust WASM plugin..."
# Navigate to workspace root if not there
cd "$(dirname "$0")/.."

# Build the WASM plugin
cargo build --manifest-path plugins/swc/rust/Cargo.toml --target wasm32-wasip1 --release

WASM_PATH="$(pwd)/target/wasm32-wasip1/release/graphox_swc_plugin.wasm"

if [ ! -f "$WASM_PATH" ]; then
    echo "Error: WASM plugin not found at $WASM_PATH"
    echo "Make sure you run this script from the project root after building with 'cargo build --target wasm32-wasip1 --release'."
    exit 1
fi

echo ""
echo "✓ Local SWC plugin development setup complete!"
echo ""
echo "To use your local WASM build, set the following environment variable:"
echo "  export GRAPHOX_SWC_PLUGIN_PATH=\"$WASM_PATH\""
echo ""
echo "After making changes to the Rust code, simply run:"
echo "  cargo build --manifest-path plugins/swc/rust/Cargo.toml --target wasm32-wasip1 --release"
echo "The changes will be automatically picked up by any process using the environment variable."
echo ""
echo "If you are using Rsbuild or Next.js, make sure to restart your dev server after changing the WASM file."
