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
            if f.endswith(".ts") or f.endswith(".json"):
                rel_dir = os.path.relpath(root, temp_out)
                target_dir = os.path.normpath(os.path.join(baseline_dir, rel_dir))
                if not os.path.exists(target_dir):
                    os.makedirs(target_dir)
                
                if f.endswith(".codegen.ts"):
                    stem = f.replace(".codegen.ts", "")
                    baseline_name = stem + ".expected.ts"
                elif f.endswith(".ts"):
                    stem = f.replace(".ts", "")
                    baseline_name = stem + ".expected.ts"
                elif f.endswith(".json"):
                    stem = f.replace(".json", "")
                    baseline_name = stem + ".expected.json"
                else:
                    baseline_name = f

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
        ("tests/fixtures/suffixes", "tests/baselines/suffixes"),
        ("tests/fixtures/operation_suffixes", "tests/baselines/operation_suffixes"),
        ("tests/fixtures/duplicate_fragment_fields", "tests/baselines/duplicate_fragment_fields"),
        ("tests/fixtures/duplicate_type_fields", "tests/baselines/duplicate_type_fields"),
        ("tests/fixtures/fragment_masking", "tests/baselines/fragment_masking"),
        ("tests/fixtures/fragment_document_suffix", "tests/baselines/fragment_document_suffix"),
        ("tests/fixtures/multi_schema_import_caching", "tests/baselines/multi_schema_import_caching"),
        ("tests/fixtures/permissions", "tests/baselines/permissions"),
        ("tests/fixtures/swc_plugin", "tests/baselines/swc_plugin"),
        ("tests/fixtures/include_strip", "tests/baselines/include_strip"),
        ("tests/fixtures/multi_project_isolation", "tests/baselines/multi_project_isolation"),
    ]

    for fixture, baseline in tasks:
        update_baselines(fixture, baseline)

    # Cleanup temp directory
    temp_out = os.path.join(ROOT, "temp_gen_out")
    if os.path.exists(temp_out):
        shutil.rmtree(temp_out)

if __name__ == "__main__":
    main()
