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

# Update version in all workspace crates
for crate in crates/graphox-*; do
    if [ -f "$crate/Cargo.toml" ]; then
        sed -i.bak "s/^version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" "$crate/Cargo.toml"
        rm "$crate/Cargo.toml.bak"
    fi
done

# Update version in SWC plugin Rust crate Cargo.toml
if [ -f "plugins/swc/rust/Cargo.toml" ]; then
    sed -i.bak "s/^version = \"$CURRENT_VERSION\"/version = \"$NEW_VERSION\"/" plugins/swc/rust/Cargo.toml
    rm plugins/swc/rust/Cargo.toml.bak
fi

# Update version in SWC plugin Node.js package.json
if [ -f "plugins/swc/node/package.json" ]; then
    sed -i.bak "s/\"version\": \"$CURRENT_VERSION\"/\"version\": \"$NEW_VERSION\"/" plugins/swc/node/package.json
    rm plugins/swc/node/package.json.bak
fi

# Update version in Babel plugin package.json
if [ -f "plugins/babel/package.json" ]; then
    sed -i.bak "s/\"version\": \"$CURRENT_VERSION\"/\"version\": \"$NEW_VERSION\"/" plugins/babel/package.json
    rm plugins/babel/package.json.bak
fi

# Update version in VSCode extension package.json
if [ -f "editors/vscode/package.json" ]; then
    sed -i.bak "s/\"version\": \"$CURRENT_VERSION\"/\"version\": \"$NEW_VERSION\"/" editors/vscode/package.json
    rm editors/vscode/package.json.bak
fi

# Update version in NPM CLI package.json and its optionalDependencies
if [ -f "npm/graphox-cli/package.json" ]; then
    # Update main version
    sed -i.bak "s/\"version\": \"$CURRENT_VERSION\"/\"version\": \"$NEW_VERSION\"/" npm/graphox-cli/package.json
    # Update optionalDependencies versions
    sed -i.bak "s/\"@soundtrack\/graphox-\(.*\)\": \"$CURRENT_VERSION\"/\"@soundtrack\/graphox-\1\": \"$NEW_VERSION\"/g" npm/graphox-cli/package.json
    rm npm/graphox-cli/package.json.bak
fi

# Update Cargo.lock
cargo update -p graphox 2>/dev/null || true
cargo update -p graphox-swc-plugin 2>/dev/null || true

# Commit changes
git add Cargo.toml Cargo.lock
git add crates/graphox-*/Cargo.toml
if [ -f "plugins/swc/rust/Cargo.toml" ]; then
    git add plugins/swc/rust/Cargo.toml
fi
if [ -f "plugins/swc/node/package.json" ]; then
    git add plugins/swc/node/package.json
fi
if [ -f "plugins/babel/package.json" ]; then
    git add plugins/babel/package.json
fi
if [ -f "editors/vscode/package.json" ]; then
    git add editors/vscode/package.json
fi
if [ -f "npm/graphox-cli/package.json" ]; then
    git add npm/graphox-cli/package.json
fi
git commit -m "chore: bump version to $NEW_VERSION"

# Create and push tag
git tag -a "v$NEW_VERSION" -m "Release v$NEW_VERSION"

echo "Version bumped to $NEW_VERSION"
echo "Changes committed and tagged as v$NEW_VERSION"
echo ""

PUSH_CMD="git push && git push origin v$NEW_VERSION"

# Copy to clipboard using OSC 52 if supported by terminal
if [ -t 1 ]; then
    printf "\033]52;c;$(printf "%s" "$PUSH_CMD" | base64 | tr -d '\n')\a"
    echo "(Command copied to clipboard)"
fi

echo "To push to remote, run:"
echo "  $PUSH_CMD"
echo ""
echo "Or to push everything at once:"
echo "  git push && git push --tags"
