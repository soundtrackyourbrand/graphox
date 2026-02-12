#!/usr/bin/env python3
import os
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BASELINES_DIR = os.path.join(ROOT, "tests", "baselines")

TSCONFIG = """
{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "node",
    "strict": true,
    "skipLibCheck": true,
    "esModuleInterop": true,
    "allowSyntheticDefaultImports": true,
    "allowImportingTsExtensions": true,
    "noEmit": true,
    "paths": {
      "@workspace/types": ["./types.ts"],
      "@my/*": ["./mocks/*"],
      "@workspace/*": ["./mocks/*"]
    }
  }
}
"""

PACKAGE_JSON = """
{
  "name": "graphox-baseline-verify",
  "version": "1.0.0",
  "dependencies": {
    "@graphql-typed-document-node/core": "latest",
    "graphql": "latest",
    "typescript": "latest",
    "@apollo/client": "latest"
  }
}
"""


def verify_baseline(name, path):
    print(f"Verifying baseline: {name}")
    with tempfile.TemporaryDirectory() as tmpdir:
        # Copy baseline files and rename .expected.ts -> .ts
        # Since we fixed update_baselines.py to preserve .codegen suffix, this is now simple
        for root, dirs, files in os.walk(path):
            rel_dir = os.path.relpath(root, path)
            target_dir = os.path.join(tmpdir, rel_dir)
            os.makedirs(target_dir, exist_ok=True)

            for f in files:
                if f.endswith(".expected.ts"):
                    new_name = f.replace(".expected.ts", ".ts")
                    shutil.copy2(
                        os.path.join(root, f), os.path.join(target_dir, new_name)
                    )
                elif f.endswith(".expected.json"):
                    new_name = f.replace(".expected.json", ".json")
                    shutil.copy2(
                        os.path.join(root, f), os.path.join(target_dir, new_name)
                    )
                else:
                    shutil.copy2(os.path.join(root, f), os.path.join(target_dir, f))

        # Write config files
        with open(os.path.join(tmpdir, "tsconfig.json"), "w") as f:
            f.write(TSCONFIG)
        with open(os.path.join(tmpdir, "package.json"), "w") as f:
            f.write(PACKAGE_JSON)

        # Ensure a dummy types.ts exists if it's expected by paths
        types_path = os.path.join(tmpdir, "types.ts")
        if not os.path.exists(types_path):
            with open(types_path, "w") as f:
                f.write("export type Status = 'ACTIVE' | 'INACTIVE';\n")
        
        # Create mocks directory for @my/* and @workspace/*
        mocks_dir = os.path.join(tmpdir, "mocks")
        os.makedirs(mocks_dir, exist_ok=True)
        for mock_file in ["schema.ts", "base-package.ts", "ext-package.ts", "graphql-schema.ts", "suffixes.ts"]:
            with open(os.path.join(mocks_dir, mock_file), "w") as f:
                f.write("export type UserStatus = 'ACTIVE' | 'INACTIVE';\n")
                f.write("export type Priority = 'LOW' | 'HIGH';\n")
                f.write("export type Status = 'ACTIVE' | 'INACTIVE';\n")
                f.write("export type GetUserQuery = { __typename: 'Query' };\n")
                f.write("export type GetUserQueryVariables = {};\n")
                f.write("export type MyEnum = 'A' | 'B';\n")
                f.write("export type Role = 'ADMIN' | 'USER';\n")
                f.write("export type UserFieldsFrag = { id: string };\n")

        # Create local mock files for permissions and project_import
        # permissions expects ./schema.types
        # project_import expects ../a/fragments.codegen
        os.makedirs(os.path.join(tmpdir, "__generated__"), exist_ok=True)
        with open(os.path.join(tmpdir, "__generated__", "schema.types.ts"), "w") as f:
            f.write("export type Status = 'ACTIVE' | 'INACTIVE';\n")
            f.write("export type PostPermissions = { read: boolean };\n")
            f.write("export type UserPermissions = { read: boolean };\n")
        
        os.makedirs(os.path.join(tmpdir, "a"), exist_ok=True)
        with open(os.path.join(tmpdir, "a", "fragments.codegen.ts"), "w") as f:
            f.write("export type MyFragment = { id: string };\n")
            f.write("export type UserFields = { id: string };\n")

        # Install dependencies only if node_modules doesn't exist in a shared location
        # or use a persistent node_modules in the ROOT
        shared_node_modules = os.path.join(
            ROOT, "scripts", "baseline_verify_node_modules"
        )
        os.makedirs(shared_node_modules, exist_ok=True)

        target_node_modules = os.path.join(tmpdir, "node_modules")

        if not os.path.exists(
            os.path.join(shared_node_modules, "@graphql-typed-document-node")
        ):
            print("  Installing dependencies (first time)...")
            # Create a dummy package.json in shared_node_modules to install there
            with open(os.path.join(shared_node_modules, "package.json"), "w") as f:
                f.write(PACKAGE_JSON)
            subprocess.run(
                ["pnpm", "install", "--shamefully-hoist", "--silent"],
                cwd=shared_node_modules,
                check=True,
            )

        # Symlink shared node_modules/node_modules to target_node_modules
        os.symlink(os.path.join(shared_node_modules, "node_modules"), target_node_modules)

        # Run tsc
        print("  Running tsc...")
        result = subprocess.run(
            ["pnpm", "exec", "tsc", "--noEmit"],
            cwd=tmpdir,
            capture_output=True,
            text=True,
        )

        if result.returncode != 0:
            print(f"  ❌ Validation FAILED for {name}")
            print(result.stdout)
            print(result.stderr)
            return False

        print(f"  ✅ Validation PASSED for {name}")
        return True


def main():
    failed = []
    baselines = [
        d
        for d in os.listdir(BASELINES_DIR)
        if os.path.isdir(os.path.join(BASELINES_DIR, d))
    ]

    # We can filter to specific ones if needed, or run all
    for name in sorted(baselines):
        path = os.path.join(BASELINES_DIR, name)
        if not verify_baseline(name, path):
            failed.append(name)

    if failed:
        print(f"\nSummary: {len(failed)} baselines failed validation:")
        for name in failed:
            print(f"  - {name}")
        sys.exit(1)
    else:
        print("\nSummary: All baselines passed TSC validation!")


if __name__ == "__main__":
    main()
