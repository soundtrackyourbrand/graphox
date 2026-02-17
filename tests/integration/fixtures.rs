use std::path::Path;
use std::process::Command;

use graphox_core::config;
use graphox_core::schema_cache;

#[test]
#[ntest::timeout(250)]
fn test_cli_codegen_baselines() {
    run_baseline_test("tests/fixtures/codegen", "tests/baselines/codegen", None);
}

#[test]
#[ntest::timeout(250)]
fn test_cli_operation_suffixes_baselines() {
    run_baseline_test(
        "tests/fixtures/operation_suffixes",
        "tests/baselines/operation_suffixes",
        None,
    );
}

#[test]
#[ntest::timeout(250)]
fn test_cli_schema_import_baselines() {
    run_baseline_test(
        "tests/fixtures/schema_import",
        "tests/baselines/schema_import",
        None,
    );
}

#[test]
#[ntest::timeout(250)]
fn test_cli_project_import_baselines() {
    run_baseline_test(
        "tests/fixtures/project_import",
        "tests/baselines/project_import",
        None,
    );
}

#[test]
#[ntest::timeout(250)]
fn test_cli_multi_schema_import_baselines() {
    run_baseline_test(
        "tests/fixtures/multi_schema_import",
        "tests/baselines/multi_schema_import",
        None,
    );
}

#[test]
#[ntest::timeout(250)]
fn test_cli_multi_schema_import_superset_baselines() {
    run_baseline_test(
        "tests/fixtures/multi_schema_import_superset",
        "tests/baselines/multi_schema_import_superset",
        None,
    );
}

#[test]
#[ntest::timeout(250)]
fn test_cli_multi_schema_import_caching_baselines() {
    run_baseline_test(
        "tests/fixtures/multi_schema_import_caching",
        "tests/baselines/multi_schema_import_caching",
        None,
    );
}

#[test]
#[ntest::timeout(250)]
fn test_cli_multi_schema_two_imports_baselines() {
    run_baseline_test(
        "tests/fixtures/multi_schema_two_imports",
        "tests/baselines/multi_schema_two_imports",
        None,
    );
}

#[test]
#[ntest::timeout(250)]
fn test_cli_public_test_baselines() {
    run_baseline_test(
        "tests/fixtures/public_test",
        "tests/baselines/public_test",
        None,
    );
}

#[test]
#[ntest::timeout(250)]
fn test_cli_fragment_ast_baselines() {
    run_baseline_test(
        "tests/fixtures/fragment_ast",
        "tests/baselines/fragment_ast",
        None,
    );
}

#[test]
#[ntest::timeout(250)]
fn test_cli_entrypoint_baselines() {
    run_baseline_test(
        "tests/fixtures/entrypoint",
        "tests/baselines/entrypoint",
        Some("gen"),
    );
}

#[test]
#[ntest::timeout(250)]
fn test_cli_aliases_baselines() {
    run_baseline_test("tests/fixtures/aliases", "tests/baselines/aliases", None);
}

#[test]
#[ntest::timeout(250)]
fn test_cli_permissions_baselines() {
    run_baseline_test(
        "tests/fixtures/permissions",
        "tests/baselines/permissions",
        None,
    );
}

#[test]
#[ntest::timeout(250)]
fn test_cli_possible_types_baselines() {
    run_baseline_test(
        "tests/fixtures/possible_types",
        "tests/baselines/possible_types",
        None,
    );
}

#[test]
#[ntest::timeout(250)]
fn test_cli_swc_plugin_baselines() {
    run_baseline_test(
        "tests/fixtures/swc_plugin",
        "tests/baselines/swc_plugin",
        Some("gen"),
    );
}

#[test]
#[ntest::timeout(250)]
fn test_cli_suffixes_baselines() {
    run_baseline_test("tests/fixtures/suffixes", "tests/baselines/suffixes", None);
}

#[test]
#[ntest::timeout(250)]
fn test_cli_re_exports_baselines() {
    run_baseline_test(
        "tests/fixtures/re_exports",
        "tests/baselines/re_exports",
        None,
    );
}

#[test]
#[ntest::timeout(250)]
fn test_cli_naming_convention_pascal_case_baselines() {
    run_baseline_test(
        "tests/fixtures/naming_convention",
        "tests/baselines/naming_convention_pascal_case",
        None,
    );
}

#[test]
#[ntest::timeout(250)]
fn test_cli_naming_convention_preserve_baselines() {
    run_baseline_test(
        "tests/fixtures/naming_convention_preserve",
        "tests/baselines/naming_convention_preserve",
        None,
    );
}

#[test]
#[ntest::timeout(250)]
fn test_cli_inline_fragments_baselines() {
    run_baseline_test(
        "tests/fixtures/inline_fragments",
        "tests/baselines/inline_fragments",
        None,
    );
}

#[test]
#[ntest::timeout(250)]
fn test_cli_output_types_baselines() {
    run_baseline_test(
        "tests/fixtures/output_types",
        "tests/baselines/output_types",
        None,
    );
}

#[test]
#[ntest::timeout(250)]
fn test_cli_typename_strictness_baselines() {
    run_baseline_test(
        "tests/fixtures/typename_strictness",
        "tests/baselines/typename_strictness",
        None,
    );
}

#[test]
#[ntest::timeout(10000)]
fn test_cli_fragment_masking_baselines() {
    run_baseline_test(
        "tests/fixtures/fragment_masking",
        "tests/baselines/fragment_masking",
        None,
    );
}

#[test]
#[ntest::timeout(10000)]
fn test_cli_fragment_document_suffix_baselines() {
    run_baseline_test(
        "tests/fixtures/fragment_document_suffix",
        "tests/baselines/fragment_document_suffix",
        None,
    );
}

#[test]
#[ntest::timeout(10000)]
fn test_cli_duplicate_type_fields() {
    run_baseline_test(
        "tests/fixtures/duplicate_type_fields",
        "tests/baselines/duplicate_type_fields",
        None,
    );
}

#[test]
#[ntest::timeout(10000)]
fn test_cli_duplicate_fragment_fields_baselines() {
    run_baseline_test(
        "tests/fixtures/duplicate_fragment_fields",
        "tests/baselines/duplicate_fragment_fields",
        None,
    );
}

#[test]
#[ntest::timeout(10000)]
fn test_cli_include_strip_baselines() {
    run_baseline_test(
        "tests/fixtures/include_strip",
        "tests/baselines/include_strip",
        None,
    );
}

pub(crate) fn run_baseline_test(
    fixture_dir_str: &str,
    baseline_dir_str: &str,
    output_dir_param: Option<&str>,
) {
    config::clear_globset_cache();
    schema_cache::clear_memory_cache();
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let fixture_dir = Path::new(fixture_dir_str);
    let baseline_dir = Path::new(baseline_dir_str);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!(
        "graphox_baselines_{}_{}",
        fixture_dir_str.replace("/", "_"),
        timestamp
    ));
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            if ty.is_dir() {
                copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
            } else {
                std::fs::copy(entry.path(), dst.join(entry.file_name()))?;
            }
        }
        Ok(())
    }

    copy_dir_all(fixture_dir, &temp_dir).expect("Failed to copy fixture to temp");

    let output_dir = output_dir_param.unwrap_or("__generated__");

    let output = Command::new(bin_path)
        .arg("codegen")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Codegen command failed for {}: {}",
        fixture_dir_str,
        String::from_utf8_lossy(&output.stderr)
    );

    let mut stack = vec![temp_dir.clone()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().unwrap() == "gen"
                    || path.file_name().unwrap() == "__generated__"
                {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) == Some("graphql") {
                let rel_to_fixture = path.strip_prefix(&temp_dir).unwrap();
                let file_stem = rel_to_fixture.file_stem().unwrap().to_str().unwrap();
                let parent = rel_to_fixture.parent().unwrap();

                let expected_path = baseline_dir
                    .join(parent)
                    .join(format!("{}.expected.ts", file_stem));

                if !expected_path.exists() {
                    continue;
                }

                let codegen_path = temp_dir
                    .join(output_dir)
                    .join(parent)
                    .join(format!("{}.codegen.ts", file_stem));

                assert!(
                    codegen_path.exists(),
                    "Codegen file {:?} was not created in {}/{}",
                    codegen_path,
                    fixture_dir_str,
                    output_dir
                );

                let actual = std::fs::read_to_string(&codegen_path).unwrap();
                let expected = std::fs::read_to_string(&expected_path).unwrap();

                let actual_norm = actual
                    .trim()
                    .replace("\r\n", "\n")
                    .replace("\\\\", "/")
                    .replace("\\", "/");
                let expected_norm = expected
                    .trim()
                    .replace("\r\n", "\n")
                    .replace("\\\\", "/")
                    .replace("\\", "/");

                if actual_norm != expected_norm {
                    println!("--- ACTUAL ({:?}) ---", path);
                    println!("{}", actual);
                    println!("--- EXPECTED ---");
                    println!("{}", expected);
                    panic!("Codegen mismatch for {:?} in {}", path, fixture_dir_str);
                }
            }
        }
    }

    verify_all_baseline_files(baseline_dir, &temp_dir, output_dir);

    std::fs::remove_dir_all(temp_dir).ok();
}

fn verify_all_baseline_files(baseline_dir: &Path, temp_dir: &Path, output_dir: &str) {
    fn verify_files_recursive(
        baseline_root: &Path,
        temp_dir: &Path,
        output_dir: &str,
        current_baseline_dir: &Path,
    ) {
        for entry in std::fs::read_dir(current_baseline_dir).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();

            if path.is_dir() {
                verify_files_recursive(baseline_root, temp_dir, output_dir, &path);
                continue;
            }

            let ext = path.extension().and_then(|s| s.to_str());
            if ext != Some("ts") && ext != Some("json") {
                continue;
            }

            let file_stem = path.file_stem().unwrap().to_str().unwrap();
            if !file_stem.ends_with(".expected") {
                continue;
            }

            let actual_name = &file_stem[..file_stem.len() - ".expected".len()];

            // Normalize paths for prefix stripping on Windows
            let canon_path = path
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap_or_else(|_| path.parent().unwrap().to_path_buf());
            let canon_baseline = baseline_root
                .canonicalize()
                .unwrap_or_else(|_| baseline_root.to_path_buf());

            let rel_path = canon_path
                .strip_prefix(&canon_baseline)
                .unwrap_or_else(|_| path.parent().unwrap().strip_prefix(baseline_root).unwrap());

            let possible_extensions = if ext == Some("ts") {
                vec!["codegen.ts", "ts"]
            } else {
                vec!["json"]
            };

            let mut actual_path = None;

            // Try EXACT relative path first
            for e in possible_extensions.iter() {
                let p = temp_dir
                    .join(rel_path)
                    .join(format!("{}.{}", actual_name, e));
                if p.exists() {
                    actual_path = Some(p);
                    break;
                }
            }

            // Try in output_dir as fallback
            if actual_path.is_none() {
                let has_output_dir_in_rel =
                    rel_path.components().any(|c| c.as_os_str() == output_dir);
                if !has_output_dir_in_rel {
                    for e in possible_extensions {
                        let p = temp_dir
                            .join(output_dir)
                            .join(rel_path)
                            .join(format!("{}.{}", actual_name, e));
                        if p.exists() {
                            actual_path = Some(p);
                            break;
                        }
                    }
                }
            }

            let actual_path = actual_path.unwrap_or_else(|| {
                panic!(
                    "Baseline file {:?} has no corresponding codegen output in temp_dir={:?} (rel_path={:?}, output_dir={:?})",
                    path,
                    temp_dir,
                    rel_path,
                    output_dir
                )
            });

            let actual = std::fs::read_to_string(&actual_path).unwrap();
            let expected = std::fs::read_to_string(&path).unwrap();

            let actual_norm = actual.trim().replace("\r\n", "\n").replace("\\\\", "/");
            let expected_norm = expected.trim().replace("\r\n", "\n").replace("\\\\", "/");

            if ext == Some("json") {
                let actual_v: serde_json::Value = match serde_json::from_str(&actual_norm) {
                    Ok(v) => v,
                    Err(e) => {
                        println!("Failed to parse actual JSON from {:?}: {}", actual_path, e);
                        println!("Content: {}", actual_norm);
                        panic!("JSON parse error for actual output");
                    }
                };
                let expected_v: serde_json::Value = match serde_json::from_str(&expected_norm) {
                    Ok(v) => v,
                    Err(e) => {
                        println!("Failed to parse expected JSON from {:?}: {}", path, e);
                        println!("Content: {}", expected_norm);
                        panic!("JSON parse error for expected baseline");
                    }
                };
                assert_eq!(actual_v, expected_v, "Baseline mismatch for {:?}", path);
            } else {
                let actual_norm = actual_norm.replace("\\", "/");
                let expected_norm = expected_norm.replace("\\", "/");
                assert_eq!(
                    actual_norm, expected_norm,
                    "Baseline mismatch for {:?}",
                    path
                );
            }
        }
    }

    verify_files_recursive(baseline_dir, temp_dir, output_dir, baseline_dir);
}
