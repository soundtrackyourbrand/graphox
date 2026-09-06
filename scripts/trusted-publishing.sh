#!/usr/bin/env bash
# Configure and verify npm trusted publishing for every package this repo ships.
#
# Trusted publishing authenticates the release workflow to npm over OIDC instead
# of a long-lived token, and attests provenance as a side effect.
#
#   ./scripts/trusted-publishing.sh configure   # attach a publisher to each package
#   ./scripts/trusted-publishing.sh verify      # read each one back from the registry
#
# Requires an npm login with write access to the @graphox scope and account-level
# 2FA. Tokens that bypass 2FA cannot configure a trusted publisher.
#
# A package must already exist on the registry before a publisher can be attached
# (npm/cli#8544). A newly added platform target therefore needs one token-based
# publish before it can appear here.
#
# Set NPM to run a different npm than the one on PATH, e.g. NPM="npx --yes npm@11".

set -uo pipefail

REPO="soundtrackyourbrand/graphox"
WORKFLOW="release.yml"
NPM="${NPM:-npm}"
REQUIRED_NPM="11.15.0"

PACKAGES=(
  "@graphox/swc-plugin"
  "@graphox/babel-plugin"
  "@graphox/cli"
  "@graphox/linux-x64"
  "@graphox/linux-arm64"
  "@graphox/darwin-x64"
  "@graphox/darwin-arm64"
  "@graphox/win32-x64"
  "@graphox/win32-arm64"
)

check_npm_version() {
  local have
  have=$($NPM --version 2>/dev/null | tail -1)
  if [ -z "$have" ]; then
    echo "Could not determine the npm version. Set NPM to a working npm." >&2
    exit 1
  fi
  # `npm trust` needs $REQUIRED_NPM for the --allow-publish permission flags.
  if [ "$(printf '%s\n%s\n' "$REQUIRED_NPM" "$have" | sort -V | head -1)" != "$REQUIRED_NPM" ]; then
    echo "npm $have is too old; trusted publishing needs $REQUIRED_NPM or newer." >&2
    echo "Re-run with a newer npm, e.g. NPM=\"npx --yes npm@11\" $0 $*" >&2
    exit 1
  fi
}

# A connection cannot be edited, only deleted and recreated, so the registry
# answers 409 for a package that already has a matching one. That is the
# desired end state, not a failure: report it and carry on.
configure() {
  local fail=0 out
  for pkg in "${PACKAGES[@]}"; do
    out=$($NPM trust github "$pkg" \
      --repo "$REPO" \
      --file "$WORKFLOW" \
      --allow-publish \
      --yes 2>&1)
    if [ $? -eq 0 ]; then
      echo "added     $pkg"
    elif printf '%s' "$out" | grep -q 'E409\|409 Conflict'; then
      echo "exists    $pkg"
    else
      echo "FAIL      $pkg"
      printf '%s\n' "$out" | sed 's/^/          /'
      fail=1
    fi
    # The registry rate-limits bulk configuration.
    sleep 2
  done
  return $fail
}

# npm does not validate a connection when it is saved, so a wrong repository or
# workflow filename stays invisible until a release fails to authenticate.
verify() {
  local fail=0 out
  for pkg in "${PACKAGES[@]}"; do
    out=$($NPM trust list "$pkg" 2>&1)
    if printf '%s' "$out" | grep -q "$REPO" && printf '%s' "$out" | grep -q "$WORKFLOW"; then
      echo "ok    $pkg"
    else
      echo "FAIL  $pkg"
      printf '%s\n' "$out" | sed 's/^/        /'
      fail=1
    fi
  done
  return $fail
}

check_npm_version
case "${1:-}" in
  configure) configure ;;
  verify)    verify ;;
  *) echo "usage: $0 {configure|verify}" >&2; exit 2 ;;
esac
