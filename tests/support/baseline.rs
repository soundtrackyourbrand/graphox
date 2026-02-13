use std::path::Path;
use std::process::Command;

use graphox_core::config;
use graphox_core::schema_cache;

pub fn run_baseline_test(
    fixture_dir_str: &str,
    baseline_dir_str: &str,
    output_dir_param: Option<&str>,
) {
    config::clear_globset_cache();
    schema_cache::clear_memory_cache();
    let bin_path = env!("CARGO_BIN_EXE_graphox");

    let current_dir = std::env::current_dir().expect("Failed to get current directory");
    let fixture_dir = current_dir.join(fixture_dir_str);
    let baseline_dir = current_dir.join(baseline_dir_str);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
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

    copy_dir_all(&fixture_dir, &temp_dir).expect("Failed to copy fixture to temp");

    let _output_dir = output_dir_param.unwrap_or("__generated__");

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
                stack.push(path.clone());
            } else {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");

                let expected_ext = if ext == "ts" || ext == "tsx" {
                    ".expected.ts"
                } else if ext == "json" {
                    ".expected.json"
                } else {
                    continue;
                };

                let expected_baseline_name = format!("{}{}", file_stem, expected_ext);
                let baseline_path = baseline_dir.join(expected_baseline_name);

                let actual = std::fs::read_to_string(&path).unwrap_or_default();
                let expected = std::fs::read_to_string(&baseline_path).unwrap_or_default();

                if actual != expected {
                    panic!(
                        "Generated file {} does not match baseline.\n\nExpected:\n{}\n\nActual:\n{}\n\nTo update baselines, run: make update-baselines",
                        path.file_name().unwrap().to_str().unwrap(),
                        expected,
                        actual
                    );
                }
            }
        }
    }

    std::fs::remove_dir_all(temp_dir).ok();
}
