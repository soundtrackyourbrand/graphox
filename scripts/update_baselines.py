#!/usr/bin/env python3
import subprocess
import os
import shutil
import sys
import re

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN_PATH = os.path.join(ROOT, "target/debug/graphox")

def update_baselines(fixture_rel, baseline_rel):
    fixture_dir = os.path.join(ROOT, fixture_rel)
    baseline_dir = os.path.join(ROOT, baseline_rel)
    
    if not os.path.exists(fixture_dir):
        print(f"Skipping {fixture_rel}: directory not found")
        return

    if not os.path.exists(baseline_dir):
        os.makedirs(baseline_dir)
    
    temp_out = os.path.join(ROOT, "temp_gen_out")
    if os.path.exists(temp_out):
        shutil.rmtree(temp_out)
    os.makedirs(temp_out)
    
    print(f"Updating baselines for {fixture_rel} -> {baseline_rel}")
    
    # Temporarily modify graphox.yaml to change output_dir
    config_path = os.path.join(fixture_dir, "graphox.yaml")
    original_config = None
    
    if os.path.exists(config_path):
        with open(config_path, 'r') as f:
            original_config = f.read()
        # Replace output_dir value
        modified_config = re.sub(r'(output_dir:\s*)".*"', r'\1"' + temp_out + '"', original_config)
        with open(config_path, 'w') as f:
            f.write(modified_config)
    
    try:
        # Run codegen from fixture directory
        args = [BIN_PATH, "codegen", "."]
        result = subprocess.run(args, cwd=fixture_dir, capture_output=True, text=True)
        if result.returncode != 0:
            print(f"  FAILED to run codegen for {fixture_rel}")
            print(result.stderr)
            return
    finally:
        # Restore original config
        if original_config:
            with open(config_path, 'w') as f:
                f.write(original_config)
    
    # Copy and rename files
    updated_count = 0
    for root, dirs, files in os.walk(temp_out):
        for f in files:
            if f.endswith(".codegen.ts") or f == "graphql.ts":
                rel_dir = os.path.relpath(root, temp_out)
                target_dir = os.path.normpath(os.path.join(baseline_dir, rel_dir))
                if not os.path.exists(target_dir):
                    os.makedirs(target_dir)
                
                if f.endswith(".codegen.ts"):
                    stem = f.replace(".codegen.ts", "")
                    baseline_name = stem + ".expected.ts"
                else:
                    baseline_name = "graphql.expected.ts"

                shutil.copy(os.path.join(root, f), os.path.join(target_dir, baseline_name))
                updated_count += 1
    
    print(f"  Done. Updated {updated_count} baseline files.")

def main():
    if not os.path.exists(BIN_PATH):
        print("Error: Binary not found. Please run 'cargo build' first.")
        sys.exit(1)

    # Dictionary of fixture -> baseline mappings
    # Tuple: (fixture_path, baseline_path)
    tasks = [
        ("tests/fixtures/codegen", "tests/baselines/codegen"),
        ("tests/fixtures/project_import", "tests/baselines/project_import"),
        ("tests/fixtures/schema_import", "tests/baselines/schema_import"),
        ("tests/fixtures/multi_schema_import", "tests/baselines/multi_schema_import"),
        ("tests/fixtures/multi_schema_import_superset", "tests/baselines/multi_schema_import_superset"),
        ("tests/fixtures/public_test", "tests/baselines/public_test"),
        ("tests/fixtures/fragment_ast", "tests/baselines/fragment_ast"),
        ("tests/fixtures/entrypoint", "tests/baselines/entrypoint"),
        ("tests/fixtures/aliases", "tests/baselines/aliases"),
    ]

    for fixture, baseline in tasks:
        update_baselines(fixture, baseline)

    # Cleanup temp directory
    temp_out = os.path.join(ROOT, "temp_gen_out")
    if os.path.exists(temp_out):
        shutil.rmtree(temp_out)

if __name__ == "__main__":
    main()
