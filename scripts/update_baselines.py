#!/usr/bin/env python3
import json
import os
import re
import shutil
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN_PATH = os.path.join(ROOT, "target/debug/graphox")


def copy_dir_all(src: str, dst: str) -> None:
    """Copy directory recursively from src to dst."""
    os.makedirs(dst, exist_ok=True)
    for item in os.listdir(src):
        src_path = os.path.join(src, item)
        dst_path = os.path.join(dst, item)
        if os.path.isdir(src_path):
            copy_dir_all(src_path, dst_path)
        else:
            shutil.copy2(src_path, dst_path)


def update_baselines(fixture_rel, baseline_rel):
    fixture_dir = os.path.join(ROOT, fixture_rel)
    baseline_dir = os.path.join(ROOT, baseline_rel)

    if not os.path.exists(fixture_dir):
        print(f"Skipping {fixture_rel}: directory not found")
        return

    if not os.path.exists(baseline_dir):
        os.makedirs(baseline_dir)

    # Clean baseline directory of ALL files and subdirectories
    for item in os.listdir(baseline_dir):
        item_path = os.path.join(baseline_dir, item)
        if os.path.isfile(item_path):
            os.remove(item_path)
        elif os.path.isdir(item_path):
            shutil.rmtree(item_path)

    # Create unique temp directories
    timestamp = str(int(time.time() * 1000))
    temp_fixture = os.path.join(ROOT, f"temp_fixture_{timestamp}")
    temp_out = os.path.join(ROOT, f"temp_gen_out_{timestamp}")

    try:
        # Copy fixture to temp directory
        print(f"Updating baselines for {fixture_rel} -> {baseline_rel}")
        copy_dir_all(fixture_dir, temp_fixture)

        config_path = os.path.join(temp_fixture, "graphox.yaml")

        # Run codegen from temp fixture directory
        args = [BIN_PATH, "codegen", "."]
        result = subprocess.run(args, cwd=temp_fixture, capture_output=True, text=True)

        if result.returncode != 0:
            print(f"  FAILED to run codegen for {fixture_rel}")
            print(result.stderr)
            return

        # Copy and rename files to baselines
        updated_count = 0

        def process_files(source_root, dest_base):
            """Process files from source_root to dest_base, renaming .ts/.json files to .expected.*"""
            nonlocal updated_count

            # Generated file patterns
            generated_extensions = (".codegen.ts",)
            generated_names = (
                "graphql.ts",
                "manifest.json",
                "schema.types.ts",
                "possible-types.ts",
                "type-policies.ts",
                "apollo-shared.ts",
                "package.json",
                "pnpm-workspace.yaml",
                "schema.ts",
                "types.ts",
                "base-types.ts",
                "ext-types.ts",
                "index.ts",
            )

            for root, dirs, files in os.walk(source_root):
                for f in files:
                    is_generated = False
                    if f.endswith(generated_extensions) or f in generated_names:
                        is_generated = True

                    # Also consider anything inside an output directory (gen or __generated__)
                    rel_dir = os.path.relpath(root, source_root)
                    if "gen" in rel_dir.split(
                        os.sep
                    ) or "__generated__" in rel_dir.split(os.sep):
                        if f.endswith(".ts") or f.endswith(".json"):
                            is_generated = True

                    if not is_generated:
                        continue

                    # Calculate target path in baseline dir
                    rel_path = os.path.relpath(os.path.join(root, f), source_root)
                    target_dir = os.path.normpath(os.path.join(dest_base, rel_dir))
                    if not os.path.exists(target_dir):
                        os.makedirs(target_dir)

                    if f.endswith(".ts"):
                        stem = f.replace(".ts", "")
                        baseline_name = stem + ".expected.ts"
                    elif f.endswith(".json"):
                        stem = f.replace(".json", "")
                        baseline_name = stem + ".expected.json"
                        with open(os.path.join(root, f), "r") as jf:
                            json_data = json.load(jf)
                        with open(os.path.join(target_dir, baseline_name), "w") as jf:
                            json.dump(json_data, jf, indent=2, sort_keys=True)
                        updated_count += 1
                        continue
                    else:
                        baseline_name = f

                    shutil.copy(
                        os.path.join(root, f), os.path.join(target_dir, baseline_name)
                    )
                    updated_count += 1

        print(f"  Capturing generated output")
        process_files(temp_fixture, baseline_dir)

        print(f"  Done. Updated {updated_count} baseline files.")

    finally:
        # Cleanup temp directories
        if os.path.exists(temp_fixture):
            shutil.rmtree(temp_fixture)
        if os.path.exists(temp_out):
            shutil.rmtree(temp_out)


def main():
    if not os.path.exists(BIN_PATH):
        print("Error: Binary not found. Please run 'cargo build' first.")
        sys.exit(1)

    # Dictionary of fixture -> baseline mappings
    tasks = [
        ("tests/fixtures/codegen", "tests/baselines/codegen"),
        ("tests/fixtures/project_import", "tests/baselines/project_import"),
        ("tests/fixtures/schema_import", "tests/baselines/schema_import"),
        ("tests/fixtures/multi_schema_import", "tests/baselines/multi_schema_import"),
        (
            "tests/fixtures/multi_schema_import_superset",
            "tests/baselines/multi_schema_import_superset",
        ),
        (
            "tests/fixtures/multi_schema_two_imports",
            "tests/baselines/multi_schema_two_imports",
        ),
        ("tests/fixtures/public_test", "tests/baselines/public_test"),
        ("tests/fixtures/fragment_ast", "tests/baselines/fragment_ast"),
        ("tests/fixtures/entrypoint", "tests/baselines/entrypoint"),
        ("tests/fixtures/aliases", "tests/baselines/aliases"),
        ("tests/fixtures/suffixes", "tests/baselines/suffixes"),
        ("tests/fixtures/re_exports", "tests/baselines/re_exports"),
        ("tests/fixtures/suffix_consistency", "tests/baselines/suffix_consistency"),
        ("tests/fixtures/operation_suffixes", "tests/baselines/operation_suffixes"),
        (
            "tests/fixtures/duplicate_fragment_fields",
            "tests/baselines/duplicate_fragment_fields",
        ),
        (
            "tests/fixtures/duplicate_type_fields",
            "tests/baselines/duplicate_type_fields",
        ),
        ("tests/fixtures/fragment_masking", "tests/baselines/fragment_masking"),
        (
            "tests/fixtures/fragment_document_suffix",
            "tests/baselines/fragment_document_suffix",
        ),
        (
            "tests/fixtures/multi_schema_import_caching",
            "tests/baselines/multi_schema_import_caching",
        ),
        ("tests/fixtures/permissions", "tests/baselines/permissions"),
        ("tests/fixtures/include_strip", "tests/baselines/include_strip"),
        (
            "tests/fixtures/multi_project_isolation",
            "tests/baselines/multi_project_isolation",
        ),
        ("tests/fixtures/emit_extensions_none", "tests/baselines/emit_extensions_none"),
        ("tests/fixtures/emit_extensions_js", "tests/baselines/emit_extensions_js"),
        ("tests/fixtures/emit_extensions_ts", "tests/baselines/emit_extensions_ts"),
        ("tests/fixtures/possible_types", "tests/baselines/possible_types"),
        ("tests/fixtures/swc_plugin", "tests/baselines/swc_plugin"),
        ("tests/fixtures/output_types", "tests/baselines/output_types"),
        (
            "tests/fixtures/interface_fragment_typename",
            "tests/baselines/interface_fragment_typename",
        ),
        (
            "tests/fixtures/naming_convention",
            "tests/baselines/naming_convention_pascal_case",
        ),
        (
            "tests/fixtures/naming_convention_preserve",
            "tests/baselines/naming_convention_preserve",
        ),
        ("tests/fixtures/inline_fragments", "tests/baselines/inline_fragments"),
        ("tests/fixtures/typename_strictness", "tests/baselines/typename_strictness"),
    ]

    for fixture, baseline in tasks:
        update_baselines(fixture, baseline)


if __name__ == "__main__":
    main()
