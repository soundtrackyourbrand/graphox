#!/bin/bash
set -e

# Script to bump version, commit, tag, and push
# Usage: ./scripts/release.sh [patch|minor|major]

VERSION_TYPE=${1:-patch}

if [[ ! "$VERSION_TYPE" =~ ^(patch|minor|major)$ ]]; then
    echo "Error: Version type must be 'patch', 'minor', or 'major'"
    echo "Usage: $0 [patch|minor|major]"
    exit 1
fi

# Get current version from Cargo.toml
CURRENT_VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
echo "Current version: $CURRENT_VERSION"

# Parse version components
IFS='.' read -r -a VERSION_PARTS <<< "$CURRENT_VERSION"
MAJOR="${VERSION_PARTS[0]}"
MINOR="${VERSION_PARTS[1]}"
PATCH="${VERSION_PARTS[2]}"

# Bump version based on type
case $VERSION_TYPE in
    patch)
        PATCH=$((PATCH + 1))
        ;;
    minor)
        MINOR=$((MINOR + 1))
        PATCH=0
        ;;
    major)
        MAJOR=$((MAJOR + 1))
        MINOR=0
        PATCH=0
        ;;
esac

NEW_VERSION="${MAJOR}.${MINOR}.${PATCH}"
echo "New version: $NEW_VERSION"

# Confirm with user
read -p "Bump version from $CURRENT_VERSION to $NEW_VERSION? (y/N) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Aborted."
    exit 1
fi

# Update version in main Cargo.toml
sed -i.bak "s/^version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" Cargo.toml
rm Cargo.toml.bak

# Update version in SWC plugin Cargo.toml
if [ -f "plugins/swc/Cargo.toml" ]; then
    sed -i.bak "s/^version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" plugins/swc/Cargo.toml
    rm plugins/swc/Cargo.toml.bak
fi

# Update version in VSCode extension package.json
if [ -f "editors/vscode/package.json" ]; then
    sed -i.bak "s/\"version\": \"$CURRENT_VERSION\"/\"version\": \"$NEW_VERSION\"/" editors/vscode/package.json
    rm editors/vscode/package.json.bak
fi

# Update version in NPM CLI package.json
if [ -f "npm/graphql-rust-cli/package.json" ]; then
    sed -i.bak "s/\"version\": \"$CURRENT_VERSION\"/\"version\": \"$NEW_VERSION\"/" npm/graphql-rust-cli/package.json
    rm npm/graphql-rust-cli/package.json.bak
fi

# Update Cargo.lock
cargo update -p graphql-rust -p graphql-rust-swc-plugin 2>/dev/null || cargo update -p graphql-rust 2>/dev/null || true

# Commit changes
git add Cargo.toml Cargo.lock
if [ -f "plugins/swc/Cargo.toml" ]; then
    git add plugins/swc/Cargo.toml
fi
if [ -f "editors/vscode/package.json" ]; then
    git add editors/vscode/package.json
fi
if [ -f "npm/graphql-rust-cli/package.json" ]; then
    git add npm/graphql-rust-cli/package.json
fi
git commit -m "chore: bump version to $NEW_VERSION"

# Create and push tag
git tag -a "v$NEW_VERSION" -m "Release v$NEW_VERSION"

echo ""
echo "Version bumped to $NEW_VERSION"
echo "Changes committed and tagged as v$NEW_VERSION"
echo ""
echo "To push to remote, run:"
echo "  git push && git push origin v$NEW_VERSION"
echo ""
echo "Or to push everything at once:"
echo "  git push && git push --tags"
