#!/usr/bin/env python3
"""Typecheck the generated TypeScript in tests/baselines with tsc.

`run_baseline_test` only compares codegen output against the committed
baselines, so it cannot tell a correct baseline from a baseline that faithfully
records broken TypeScript. This script copies each baseline into a scratch
workspace, renames `*.expected.ts` back to `*.ts`, and compiles it.

Requires pnpm on PATH.
"""

import argparse
import copy
import json
import os
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BASELINES_DIR = os.path.join(ROOT, "tests", "baselines")

# The manifest is committed and its versions are pinned, so a release of
# TypeScript or graphql-js cannot turn a green baseline red without a commit
# that says so. Bump it deliberately, and refresh the lockfile alongside it with
# `pnpm install --lockfile-only`.
TOOLCHAIN_DIR = os.path.join(ROOT, "scripts", "baseline-verify")
MANIFEST_FILES = ("package.json", "pnpm-lock.yaml")

# pnpm needs the manifest beside the node_modules it installs, but a manifest
# copied into the workspace only records intent: an install that fails after the
# copy would leave the new pins looking satisfied by whatever node_modules
# survived, and the next run would typecheck against the old compiler. This is
# written last, so it is the only evidence that an install finished.
STAMP_FILE = ".pinned-toolchain"

# Installed under target/ so node_modules stays out of git and `cargo clean`
# reclaims it.
WORKSPACE_DIR = os.path.join(ROOT, "target", "baseline-verify")

BASE_TSCONFIG = {
    "compilerOptions": {
        "target": "ESNext",
        "module": "ESNext",
        # Generated imports are extensionless and resolved by a bundler in the
        # projects that consume them. node10 resolution was removed in
        # TypeScript 7 anyway.
        "moduleResolution": "bundler",
        "strict": True,
        "skipLibCheck": True,
        "esModuleInterop": True,
        "allowSyntheticDefaultImports": True,
        "allowImportingTsExtensions": True,
        "noEmit": True,
        "paths": {
            # Fixtures that emit shared schema types import them under this
            # name; it resolves to the types.ts generated alongside them.
            "@workspace/types": ["./types.ts"],
        },
    }
}


def install_dependencies():
    """Install the pinned toolchain once, into a workspace shared by every
    baseline. Returns the path to the tsc binary."""
    os.makedirs(WORKSPACE_DIR, exist_ok=True)
    bin_dir = os.path.join(WORKSPACE_DIR, "node_modules", ".bin")
    stamp_path = os.path.join(WORKSPACE_DIR, STAMP_FILE)

    def read(path):
        with open(path, "rb") as f:
            return f.read()

    pinned = {name: read(os.path.join(TOOLCHAIN_DIR, name)) for name in MANIFEST_FILES}
    stamp = b"\0".join(pinned[name] for name in MANIFEST_FILES)

    # Reinstall when the pins move, so a bump needs no manual clean.
    tsc_bin = shutil.which("tsc", path=bin_dir)
    if tsc_bin and os.path.exists(stamp_path) and read(stamp_path) == stamp:
        return tsc_bin

    print("Installing the pinned TypeScript toolchain...")
    # Dropped first, so that a failure below cannot leave a stamp behind
    # vouching for the install that did not happen.
    if os.path.exists(stamp_path):
        os.remove(stamp_path)

    # From scratch, because pnpm short-circuits on its own dependency-status
    # check when node_modules looks current: it reports "Already up to date"
    # without comparing the manifest to the lockfile, so a bumped pin would be
    # recorded as installed while the old version stayed on disk. A clean tree
    # also makes --frozen-lockfile do its job and reject a lockfile that no
    # longer matches the manifest.
    node_modules = os.path.join(WORKSPACE_DIR, "node_modules")
    if os.path.exists(node_modules):
        shutil.rmtree(node_modules)

    for name, contents in pinned.items():
        with open(os.path.join(WORKSPACE_DIR, name), "wb") as f:
            f.write(contents)

    # --shamefully-hoist because each baseline gets this node_modules
    # wholesale, without a manifest of its own for pnpm to link against. Not
    # silenced: this runs only on a first run or a pin change, and pnpm is the
    # only thing that can explain a refused lockfile.
    try:
        subprocess.run(
            ["pnpm", "install", "--frozen-lockfile", "--shamefully-hoist"],
            cwd=WORKSPACE_DIR,
            check=True,
        )
    except subprocess.CalledProcessError as e:
        sys.exit(
            f"Error: pnpm install failed ({e.returncode}). If the manifest in "
            f"{os.path.relpath(TOOLCHAIN_DIR, ROOT)} was edited, refresh the "
            "lockfile beside it with `pnpm install --lockfile-only`."
        )

    tsc_bin = shutil.which("tsc", path=bin_dir)
    if not tsc_bin:
        sys.exit(f"Error: pnpm install did not produce a tsc binary in {bin_dir}")

    with open(stamp_path, "wb") as f:
        f.write(stamp)
    return tsc_bin


def stage_baseline(path, tmpdir):
    """Copy a baseline into tmpdir, renaming .expected.* back to .*"""
    for root, _dirs, files in os.walk(path):
        target_dir = os.path.join(tmpdir, os.path.relpath(root, path))
        os.makedirs(target_dir, exist_ok=True)

        for f in files:
            if f.endswith(".expected.ts"):
                new_name = f[: -len(".expected.ts")] + ".ts"
            elif f.endswith(".expected.json"):
                new_name = f[: -len(".expected.json")] + ".json"
            else:
                new_name = f
            shutil.copy2(os.path.join(root, f), os.path.join(target_dir, new_name))


def discover_paths(tmpdir):
    """Map every package name declared in the staged baseline to its sources, so
    that cross-package imports resolve without an install step."""
    paths = copy.deepcopy(BASE_TSCONFIG["compilerOptions"]["paths"])

    for root, _dirs, files in os.walk(tmpdir):
        if "package.json" not in files:
            continue

        pkg_path = os.path.join(root, "package.json")
        try:
            with open(pkg_path, "r") as f:
                pkg = json.load(f)
        except (OSError, json.JSONDecodeError) as e:
            print(f"  Warning: failed to parse {pkg_path}: {e}")
            continue

        name = pkg.get("name")
        if not name:
            continue

        rel_to_root = os.path.relpath(root, tmpdir)
        # "main" if the package declares an entry point, the package directory
        # otherwise — tsc resolves index.ts inside it.
        if pkg.get("main"):
            target = os.path.join(rel_to_root, pkg["main"])
        else:
            target = rel_to_root
        paths[name] = ["./" + target.replace(os.sep, "/")]

    return paths


def has_typescript(path):
    return any(
        f.endswith(".expected.ts")
        for _root, _dirs, files in os.walk(path)
        for f in files
    )


def verify_baseline(name, path, tsc_bin):
    # Not every baseline is codegen output — the formatter's, for one, is
    # GraphQL. tsc has nothing to say about those.
    if not has_typescript(path):
        print(f"Skipping baseline (no TypeScript): {name}")
        return True

    print(f"Verifying baseline: {name}")
    with tempfile.TemporaryDirectory() as tmpdir:
        stage_baseline(path, tmpdir)

        tsconfig = copy.deepcopy(BASE_TSCONFIG)
        tsconfig["compilerOptions"]["paths"] = discover_paths(tmpdir)
        with open(os.path.join(tmpdir, "tsconfig.json"), "w") as f:
            json.dump(tsconfig, f, indent=2)

        # A manifest of its own would make pnpm reinstall over the shared
        # node_modules, so the baseline gets the dependencies by symlink and tsc
        # is invoked directly rather than through `pnpm exec`.
        os.symlink(
            os.path.join(WORKSPACE_DIR, "node_modules"),
            os.path.join(tmpdir, "node_modules"),
        )

        result = subprocess.run(
            [tsc_bin, "--project", "tsconfig.json"],
            cwd=tmpdir,
            capture_output=True,
            text=True,
        )

        if result.returncode != 0:
            print(f"  FAILED: {name}")
            for line in (result.stdout + result.stderr).splitlines():
                print(f"    {line}")
            return False

        print(f"  passed: {name}")
        return True


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "baselines",
        nargs="*",
        help="names of baselines to verify (default: all of them)",
    )
    args = parser.parse_args()

    available = sorted(
        d for d in os.listdir(BASELINES_DIR)
        if os.path.isdir(os.path.join(BASELINES_DIR, d))
    )

    selected = args.baselines or available
    unknown = [name for name in selected if name not in available]
    if unknown:
        sys.exit(f"Error: no such baseline: {', '.join(unknown)}")

    checked = [
        name
        for name in selected
        if has_typescript(os.path.join(BASELINES_DIR, name))
    ]

    tsc_bin = install_dependencies()

    failed = [
        name for name in selected
        if not verify_baseline(name, os.path.join(BASELINES_DIR, name), tsc_bin)
    ]

    if failed:
        print(f"\n{len(failed)} of {len(checked)} baselines failed to typecheck:")
        for name in failed:
            print(f"  - {name}")
        sys.exit(1)

    print(f"\nAll {len(checked)} baselines typecheck.")


if __name__ == "__main__":
    main()
