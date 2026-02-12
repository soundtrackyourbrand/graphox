#!/usr/bin/env python3
import os
import shutil
import subprocess
import sys
import tempfile
import json

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BASELINES_DIR = os.path.join(ROOT, "tests", "baselines")

BASE_TSCONFIG = {
    "compilerOptions": {
        "target": "ESNext",
        "module": "ESNext",
        "moduleResolution": "node",
        "strict": True,
        "skipLibCheck": True,
        "esModuleInterop": True,
        "allowSyntheticDefaultImports": True,
        "allowImportingTsExtensions": True,
        "noEmit": True,
        "paths": {
            "@workspace/types": ["./types.ts"]
        }
    }
}

PACKAGE_JSON = {
    "name": "graphox-baseline-verify",
    "version": "1.0.0",
    "dependencies": {
        "@graphql-typed-document-node/core": "latest",
        "graphql": "latest",
        "typescript": "latest",
        "@apollo/client": "latest"
    }
}

def verify_baseline(name, path):
    print(f"Verifying baseline: {name}")
    with tempfile.TemporaryDirectory() as tmpdir:
        # Copy baseline files and rename .expected.* -> .*
        for root, dirs, files in os.walk(path):
            rel_dir = os.path.relpath(root, path)
            target_dir = os.path.join(tmpdir, rel_dir)
            os.makedirs(target_dir, exist_ok=True)

            for f in files:
                if f.endswith(".expected.ts"):
                    new_name = f.replace(".expected.ts", ".ts")
                elif f.endswith(".expected.json"):
                    new_name = f.replace(".expected.json", ".json")
                else:
                    new_name = f
                shutil.copy2(os.path.join(root, f), os.path.join(target_dir, new_name))

        # Scan for package.json files to build tsconfig paths
        paths = BASE_TSCONFIG["compilerOptions"]["paths"].copy()
        
        for root, dirs, files in os.walk(tmpdir):
            if "package.json" in files:
                pkg_path = os.path.join(root, "package.json")
                try:
                    with open(pkg_path, "r") as f:
                        pkg = json.load(f)
                        pkg_name = pkg.get("name")
                        pkg_main = pkg.get("main")
                        
                        if pkg_name:
                            # Calculate relative path from tmpdir to the main file or the directory
                            rel_to_root = os.path.relpath(root, tmpdir)
                            if pkg_main:
                                # If it has a main, map to it
                                main_path = os.path.join(".", rel_to_root, pkg_main)
                                paths[pkg_name] = [main_path]
                            else:
                                # Otherwise map to the directory (index.ts or package root)
                                paths[pkg_name] = [os.path.join(".", rel_to_root)]
                except Exception as e:
                    print(f"  Warning: Failed to parse {pkg_path}: {e}")

        # Update tsconfig with discovered paths
        tsconfig = BASE_TSCONFIG.copy()
        tsconfig["compilerOptions"]["paths"] = paths

        # Write config files
        with open(os.path.join(tmpdir, "tsconfig.json"), "w") as f:
            json.dump(tsconfig, f, indent=2)
            
        # Ensure we have a top-level package.json for dependencies
        if not os.path.exists(os.path.join(tmpdir, "package.json")):
            with open(os.path.join(tmpdir, "package.json"), "w") as f:
                json.dump(PACKAGE_JSON, f, indent=2)

        # Ensure a dummy types.ts exists if it's expected by paths
        types_path = os.path.join(tmpdir, "types.ts")
        if not os.path.exists(types_path):
            with open(types_path, "w") as f:
                f.write("export type Status = 'ACTIVE' | 'INACTIVE';\n")
        
        # Shared node_modules location
        shared_node_modules = os.path.join(ROOT, "scripts", "baseline_verify_node_modules")
        os.makedirs(shared_node_modules, exist_ok=True)
        target_node_modules = os.path.join(tmpdir, "node_modules")

        if not os.path.exists(os.path.join(shared_node_modules, "node_modules", "graphql")):
            print("  Installing dependencies (first time)...")
            with open(os.path.join(shared_node_modules, "package.json"), "w") as f:
                json.dump(PACKAGE_JSON, f, indent=2)
            subprocess.run(["pnpm", "install", "--shamefully-hoist", "--silent"], cwd=shared_node_modules, check=True)

        # Symlink shared node_modules
        if not os.path.exists(target_node_modules):
            os.symlink(os.path.join(shared_node_modules, "node_modules"), target_node_modules)

        # Run tsc
        print("  Running tsc...")
        result = subprocess.run(["pnpm", "exec", "tsc", "--noEmit"], cwd=tmpdir, capture_output=True, text=True)

        if result.returncode != 0:
            print(f"  ❌ Validation FAILED for {name}")
            print(result.stdout)
            print(result.stderr)
            return False

        print(f"  ✅ Validation PASSED for {name}")
        return True

def main():
    failed = []
    baselines = [d for d in os.listdir(BASELINES_DIR) if os.path.isdir(os.path.join(BASELINES_DIR, d))]

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
