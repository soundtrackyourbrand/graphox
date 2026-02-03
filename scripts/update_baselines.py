#!/usr/bin/env python3
import subprocess
import os
import shutil
import sys

# Get the absolute path to the project root
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN_PATH = os.path.join(ROOT, "target/debug/graphql-rust")
SIMPLE_SCHEMA = os.path.join(ROOT, "tests/fixtures/simple_schema.graphql")

def update_baselines(fixture_rel, baseline_rel, use_simple_schema=False):
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
    
    # Run codegen
    args = [BIN_PATH]
    if use_simple_schema:
        args.extend(["--schema", SIMPLE_SCHEMA])
    args.extend(["codegen", ".", "--output", temp_out])
    
    try:
        subprocess.run(args, cwd=fixture_dir, check=True, capture_output=True)
    except subprocess.CalledProcessError as e:
        print(f"  FAILED to run codegen for {fixture_rel}")
        print(e.stderr.decode())
        return
    
    # Copy and rename files
    updated_count = 0
    for root, dirs, files in os.walk(temp_out):
        for f in files:
            if f.endswith(".codegen.ts"):
                # Calculate relative path from temp_out to preserve structure
                rel_dir = os.path.relpath(root, temp_out)
                target_dir = os.path.normpath(os.path.join(baseline_dir, rel_dir))
                if not os.path.exists(target_dir):
                    os.makedirs(target_dir)
                
                stem = f.replace(".codegen.ts", "")
                baseline_name = stem + ".expected.ts"
                shutil.copy(os.path.join(root, f), os.path.join(target_dir, baseline_name))
                updated_count += 1
    
    print(f"  Done. Updated {updated_count} baseline files.")

def main():
    if not os.path.exists(BIN_PATH):
        print("Error: Binary not found. Please run 'cargo build' first.")
        sys.exit(1)

    # Dictionary of fixture -> baseline mappings
    # Tuple: (fixture_path, baseline_path, use_simple_schema)
    tasks = [
        ("tests/fixtures/codegen", "tests/baselines/codegen", True),
        ("tests/fixtures/project_import", "tests/baselines/project_import", False),
        ("tests/fixtures/schema_import", "tests/baselines/schema_import", False),
        ("tests/fixtures/multi_schema_import", "tests/baselines/multi_schema_import", False),
        ("tests/fixtures/multi_schema_import_superset", "tests/baselines/multi_schema_import_superset", False),
        ("tests/fixtures/public_test", "tests/baselines/public_test", True),
    ]

    for fixture, baseline, use_schema in tasks:
        update_baselines(fixture, baseline, use_schema)

    # Cleanup temp directory
    temp_out = os.path.join(ROOT, "temp_gen_out")
    if os.path.exists(temp_out):
        shutil.rmtree(temp_out)

if __name__ == "__main__":
    main()
