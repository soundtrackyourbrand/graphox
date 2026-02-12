use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
#[ntest::timeout(30000)]
#[ignore] // This test is slow and requires node/npm and wasm32-wasip1 target
fn test_swc_cli_integration() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture_dir = root_dir.join("tests/fixtures/swc_cli");
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path();

    // Copy fixture files to temp directory
    copy_dir_all(&fixture_dir, temp_path).expect("Failed to copy fixture files");

    // 1. Run codegen
    let output = Command::new(bin_path)
        .arg("codegen")
        .arg(".")
        .current_dir(temp_path)
        .output()
        .expect("Failed to execute codegen");

    assert!(
        output.status.success(),
        "Codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // 2. Build SWC plugin to WASM
    // We assume the target wasm32-wasip1 is installed
    let build_output = Command::new("cargo")
        .arg("build")
        .arg("-p")
        .arg("graphox-swc-plugin")
        .arg("--target")
        .arg("wasm32-wasip1")
        .arg("--release")
        .current_dir(root_dir)
        .output()
        .expect("Failed to build SWC plugin");

    assert!(
        build_output.status.success(),
        "Plugin build failed: {}",
        String::from_utf8_lossy(&build_output.stderr)
    );

    let wasm_path = root_dir.join("target/wasm32-wasip1/release/graphox_swc_plugin.wasm");
    assert!(wasm_path.exists(), "WASM plugin missing at {:?}", wasm_path);

    // 3. Install SWC CLI
    println!("Installing SWC CLI...");
    fs::write(temp_path.join("package.json"), r#"{ "name": "swc-test" }"#).unwrap();

    let npm_install = Command::new("npm")
        .arg("install")
        .arg("@swc/core")
        .arg("@swc/cli")
        .current_dir(temp_path)
        .output()
        .expect("Failed to run npm install");
    assert!(npm_install.status.success());

    // 4. Create .swcrc
    let manifest_path = temp_path.join("gen/manifest.json");
    let output_dir = temp_path.join("gen");
    assert!(manifest_path.exists(), "manifest.json missing");
    let manifest_json = fs::read_to_string(&manifest_path).unwrap();
    let manifest_data: serde_json::Value = serde_json::from_str(&manifest_json).unwrap();

    let swcrc = serde_json::json!({
        "jsc": {
            "parser": {
                "syntax": "typescript",
            },
            "experimental": {
                "plugins": [
                    [
                        wasm_path.to_str().unwrap(),
                        {
                            "manifestData": manifest_data,
                            "outputDir": output_dir.to_str().unwrap()
                        }
                    ]
                ]
            }
        }
    });

    fs::write(
        temp_path.join(".swcrc"),
        serde_json::to_string_pretty(&swcrc).unwrap(),
    )
    .unwrap();

    // 5. Run SWC
    println!("Running SWC...");
    let swc_bin = temp_path.join("node_modules/@swc/cli/bin/swc.js");
    let swc_output = Command::new("node")
        .arg(swc_bin)
        .arg(temp_path.join("src/app.ts"))
        .arg("-o")
        .arg("out.js")
        .current_dir(temp_path)
        .output()
        .expect("Failed to run SWC");

    assert!(
        swc_output.status.success(),
        "SWC failed: {}\n{}",
        String::from_utf8_lossy(&swc_output.stdout),
        String::from_utf8_lossy(&swc_output.stderr)
    );

    let out_js_path = temp_path.join("out.js");
    assert!(out_js_path.exists(), "out.js missing");

    let transformed_code = fs::read_to_string(out_js_path).unwrap();

    // Verify transformation
    // It should import the document and replace the call
    assert!(transformed_code.contains("GetMeQueryDocument"));
    assert!(transformed_code.contains("../gen/src/app.codegen"));
    assert!(!transformed_code.contains("graphql("));
    assert!(!transformed_code.contains("import { graphql }"));
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}
