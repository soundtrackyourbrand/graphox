//! Slow and end-to-end integration tests.
//! These tests are marked with #[ignore] as they are slow to run.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;
#[cfg(windows)]
use std::os::windows::process::ExitStatusExt;

use graphox_core::config;
use graphox_core::schema_cache;

#[tokio::test]
#[ntest::timeout(300000)]
#[ignore] // Slow test - runs full monorepo setup with pnpm install and multiple typechecks
async fn test_cli_monorepo_e2e_typecheck_and_compare() {
    test_monorepo_typecheck_and_compare("tests/fixtures/monorepo_e2e").await;
}

async fn test_monorepo_typecheck_and_compare(fixture_dir_str: &str) {
    config::clear_globset_cache();
    schema_cache::clear_memory_cache();
    let graphox_bin_path = env!("CARGO_BIN_EXE_graphox");
    let fixture_dir = Path::new(fixture_dir_str);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("graphox_monorepo_e2e_{}", timestamp));
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir).ok();
    }
    std::fs::create_dir_all(&temp_dir).ok();

    fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let file_name = entry.file_name();
            if file_name == "node_modules" || file_name == "target" || file_name == ".turbo" {
                continue;
            }
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
            } else if let Err(e) = std::fs::copy(entry.path(), dst.join(entry.file_name())) {
                panic!(
                    "Failed to copy {:?} to {:?}: {}",
                    entry.path(),
                    dst.join(entry.file_name()),
                    e
                );
            }
        }
        Ok(())
    }

    copy_dir_all(fixture_dir, &temp_dir).expect("Failed to copy fixture to temp");

    // Clean up old gql.ts files from graphql-codegen to avoid conflicts
    println!("[Cleanup] Removing old gql.ts files...");
    fn remove_old_gql_files(dir: &Path) -> std::io::Result<()> {
        if dir.is_dir() {
            for entry in std::fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    remove_old_gql_files(&path)?;
                } else if path.file_name().is_some_and(|n| n == "gql.ts") {
                    std::fs::remove_file(&path)?;
                }
            }
        }
        Ok(())
    }
    remove_old_gql_files(&temp_dir).ok();

    // Step 1: Install dependencies
    println!("[PNPM] Installing dependencies...");
    let install_output = Command::new("pnpm")
        .args(["install", "--no-frozen-lockfile"])
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to run pnpm install");

    if !install_output.status.success() {
        eprintln!(
            "pnpm install stdout: {}",
            String::from_utf8_lossy(&install_output.stdout)
        );
        eprintln!(
            "pnpm install stderr: {}",
            String::from_utf8_lossy(&install_output.stderr)
        );
    }
    assert!(install_output.status.success(), "pnpm install failed");

    // Step 2: Run graphox codegen
    println!("[Graphox] Running codegen...");
    let graphox_output = Command::new(graphox_bin_path)
        .arg("codegen")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to execute graphox");

    assert!(
        graphox_output.status.success(),
        "Graphox codegen failed: {}",
        String::from_utf8_lossy(&graphox_output.stderr)
    );

    // Step 3: Typecheck with graphox output
    println!("[TypeScript] Typechecking with graphox output...");
    let graphox_typecheck = typecheck_monorepo(&temp_dir, "generated");
    if !graphox_typecheck.status.success() {
        eprintln!(
            "Typecheck stderr: {}",
            String::from_utf8_lossy(&graphox_typecheck.stderr)
        );
    }
    assert!(
        graphox_typecheck.status.success(),
        "Typecheck failed with graphox output: {}",
        String::from_utf8_lossy(&graphox_typecheck.stderr)
    );

    // Step 3: Run graphql-codegen
    println!("[GraphQL-Codegen] Running codegen...");
    let _graphox_generated_dir = temp_dir.join("packages").join("schema").join("generated");
    let _gqldo_generated_dir = temp_dir.join("packages").join("schema").join("generated");

    // Run graphql-codegen
    let gqldo_output = Command::new("pnpm")
        .args(["exec", "graphql-codegen", "--config", "codegen.ts"])
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to execute graphql-codegen");

    assert!(
        gqldo_output.status.success(),
        "graphql-codegen failed: {}",
        String::from_utf8_lossy(&gqldo_output.stderr)
    );

    // Step 3: Typecheck with graphql-codegen output
    // Note: This may fail because source files use graphox's API format
    println!("[TypeScript] Typechecking with graphql-codegen output...");
    let gqldo_typecheck = typecheck_monorepo(&temp_dir, "generated");
    if !gqldo_typecheck.status.success() {
        println!(
            "[Note] GraphQL-Codegen typecheck failed (expected - source files use graphox API)"
        );
        println!(
            "stdout: {}",
            String::from_utf8_lossy(&gqldo_typecheck.stdout)
        );
    }

    // Re-run graphox to restore files for comparison
    println!("[Graphox] Re-running codegen for comparison...");
    let graphox_output2 = Command::new(graphox_bin_path)
        .arg("codegen")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to execute graphox");

    assert!(
        graphox_output2.status.success(),
        "Graphox codegen failed: {}",
        String::from_utf8_lossy(&graphox_output2.stderr)
    );

    // Step 4: Compare AST outputs
    println!("[Compare] Comparing AST outputs...");
    compare_ast_outputs(&temp_dir);

    // Cleanup
    std::fs::remove_dir_all(&temp_dir).ok();
}

fn typecheck_monorepo(temp_dir: &Path, _generated_dir: &str) -> std::process::Output {
    let packages = vec!["schema", "ui-lib", "app", "app-masking", "app-graphox"];
    let mut all_output = String::new();
    let mut success = true;

    for pkg in packages {
        let pkg_dir = temp_dir.join("packages").join(pkg);
        let output = Command::new("pnpm")
            .args(["exec", "--", "tsc", "--noEmit"])
            .current_dir(&pkg_dir)
            .output()
            .unwrap_or_else(|e| {
                success = false;
                all_output.push_str(&format!("\n=== {} ===\n", pkg));
                all_output.push_str(&format!("Failed to run tsc: {}", e));
                std::process::Output {
                    status: std::process::ExitStatus::from_raw(1),
                    stdout: vec![],
                    stderr: all_output.as_bytes().to_vec(),
                }
            });

        if !output.status.success() {
            success = false;
            all_output.push_str(&format!("\n=== {} ===\n", pkg));
            all_output.push_str(&String::from_utf8_lossy(&output.stderr));
            if output.stderr.is_empty() {
                all_output.push_str(&format!(
                    "stdout: {}",
                    String::from_utf8_lossy(&output.stdout)
                ));
            }
        }
    }

    std::process::Output {
        status: if success {
            std::process::ExitStatus::from_raw(0)
        } else {
            std::process::ExitStatus::from_raw(1)
        },
        stdout: vec![],
        stderr: all_output.as_bytes().to_vec(),
    }
}

fn compare_ast_outputs(temp_dir: &Path) {
    // For each package, compare the AST documents
    let packages = vec!["ui-lib", "app", "app-masking", "app-graphox"];

    for pkg in packages {
        println!("[Compare] Running JS-based comparison for {}", pkg);

        let pkg_dir = temp_dir.join("packages").join(pkg);
        let graphox_gen_dir = pkg_dir.join("src").join("__generated__");
        let gqldo_gen_dir = if pkg == "app-graphox" {
            temp_dir
                .join("packages")
                .join("app-reference")
                .join("src")
                .join("generated")
        } else {
            pkg_dir.join("src").join("generated")
        };

        if !graphox_gen_dir.exists() || !gqldo_gen_dir.exists() {
            println!("[Compare] Skipping {} due to missing directories", pkg);
            continue;
        }

        // 1. Collect document names
        let graphox_docs = extract_all_documents(&graphox_gen_dir);
        let mut doc_names: Vec<String> = graphox_docs.keys().cloned().collect();
        doc_names.sort();

        if doc_names.is_empty() {
            println!("[Compare] No documents found in {}", pkg);
            continue;
        }

        // 2. Generate comparison script
        let cod_dir = if pkg == "app-graphox" {
            "../app-reference/src/generated"
        } else {
            "src/generated"
        };

        let script = format!(
            r#"
import {{ deepStrictEqual }} from 'node:assert';
import {{ pathToFileURL }} from 'node:url';
import {{ join }} from 'node:path';

async function run() {{
  const docNames = '{doc_names}'.split(',');
  const pkgDir = process.cwd();
  
  let exitCode = 0;
  for (const name of docNames) {{
    if (!name) continue;
    try {{
      // Find files containing this document
      const goxFile = await findFileWithExport(join(pkgDir, 'src', '__generated__'), name + 'Document');
      const codFile = await findFileWithExport(join(pkgDir, '{cod_dir}'), name + 'Document') 
                   || await findFileWithExport(join(pkgDir, '{cod_dir}'), name + 'FragmentDoc');

      if (goxFile && codFile) {{
        console.log(`Comparing ${{name}} from ${{goxFile}} against ${{codFile}}...`);
        const goxMod = await import(pathToFileURL(goxFile).href);
        const codMod = await import(pathToFileURL(codFile).href);
        
        const goxDoc = goxMod[name + 'Document'];
        const codDoc = codMod[name + 'Document'] || codMod[name + 'FragmentDoc'];
        
        if (!goxDoc || !codDoc) {{
           console.warn(`⚠️  Missing export for ${{name}}: Gox=${{!!goxDoc}}, Cod=${{!!codDoc}}`);
           continue;
        }}

        // We only compare kind and definitions
        const cleanGox = normalize(goxDoc);
        const cleanCod = normalize(codDoc);
        
        deepStrictEqual(cleanGox, cleanCod);
        console.log(`✅ ${{name}} matches`);
      }} else {{
        console.warn(`⚠️  Could not find files for ${{name}} (Gox: ${{!!goxFile}}, Cod: ${{!!codFile}})`);
      }}
    }} catch (e) {{
      console.error(`❌ ${{name}} mismatch or error!`);
      console.error(e.message);
      exitCode = 1;
    }}
  }}
  process.exit(exitCode);
}}

function normalize(node) {{
  if (!node || typeof node !== 'object') return node;
  if (Array.isArray(node)) {{
    // Filter out @public directive
    const filtered = node.filter(item => !(item && item.kind === 'Directive' && item.name?.value === 'public'));
    const mapped = filtered.map(normalize);
    if (mapped.length === 0) return undefined;
    // Sort if it makes sense (definitions and selections)
    if (mapped.length > 1 && (mapped[0].kind === 'Field' || mapped[0].kind === 'FragmentSpread' || mapped[0].kind === 'InlineFragment' || mapped[0].kind === 'FragmentDefinition' || mapped[0].kind === 'OperationDefinition' || mapped[0].kind === 'VariableDefinition')) {{
      return mapped.sort((a, b) => {{
        const nameA = a.name?.value || a.variable?.name?.value || '';
        const nameB = b.name?.value || b.variable?.name?.value || '';
        return nameA.localeCompare(nameB);
      }});
    }}
    return mapped;
  }}
  const result = {{}};
  for (const key of Object.keys(node).sort()) {{
    if (key === 'loc') continue; // Always ignore locations
    const val = normalize(node[key]);
    if (val !== undefined) {{
      result[key] = val;
    }}
  }}
  return result;
}}

async function findFileWithExport(dir, exportName) {{
  const {{ readdir, readFile, stat }} = await import('node:fs/promises');
  try {{
    const entries = await readdir(dir);
    for (const entry of entries) {{
      const fullPath = join(dir, entry);
      const s = await stat(fullPath);
      if (s.isDirectory()) {{
        const found = await findFileWithExport(fullPath, exportName);
        if (found) return found;
      }} else if (entry.endsWith('.ts')) {{
        const content = await readFile(fullPath, 'utf8');
        if (content.includes(`export const ${{exportName}}`)) {{
          return fullPath;
        }}
      }}
    }}
  }} catch (e) {{}}
  return null;
}}

run();
"#,
            doc_names = doc_names.join(","),
            cod_dir = cod_dir
        );

        let script_path = pkg_dir.join("compare_ast.ts");
        std::fs::write(&script_path, script).expect("Failed to write comparison script");

        // 3. Execute comparison script
        let output = std::process::Command::new("pnpm")
            .arg("exec")
            .arg("tsx")
            .arg("compare_ast.ts")
            .current_dir(&pkg_dir)
            .output()
            .expect("Failed to execute comparison script");

        if !output.status.success() {
            println!("=== {} AST Comparison Failure ===", pkg);
            println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
            println!("stderr: {}", String::from_utf8_lossy(&output.stderr));
            panic!("AST comparison failed for package {}", pkg);
        } else {
            println!("[Compare] JS-based comparison for {} PASSED", pkg);
        }
    }

    println!("[Compare] All JS-based comparisons complete!");
}

fn extract_all_documents(dir: &Path) -> HashMap<String, String> {
    let mut all_docs = HashMap::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let sub_docs = extract_all_documents(&path);
                all_docs.extend(sub_docs);
            } else if path.extension().is_some_and(|ext| ext == "ts") {
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                let docs = extract_documents(&content);
                all_docs.extend(docs);
            }
        }
    }
    all_docs
}

fn extract_documents(content: &str) -> HashMap<String, String> {
    let mut docs = HashMap::new();

    // Look for Document variable declarations
    // Graphox: export const UserCardQueryDocument = ...
    // GraphQL-Codegen: export const UserCardQueryDocument: TypedDocumentNode = ...
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("export const ")
            && line.contains("Document")
            && (line.contains("= ") || line.contains(": "))
        {
            // Extract document name
            if let Some(name_start) = line.find("export const ") {
                let after_export = &line[name_start + "export const ".len()..];
                if let Some(eq_pos) = after_export.find('=') {
                    let mut name = if let Some(colon_pos) = after_export.find(':') {
                        if colon_pos < eq_pos {
                            after_export[..colon_pos].trim()
                        } else {
                            after_export[..eq_pos].trim()
                        }
                    } else {
                        after_export[..eq_pos].trim()
                    };

                    if name.ends_with("Document") {
                        name = &name[..name.len() - "Document".len()];
                    }

                    // Try to find the operation definition
                    if let Some(def_start) = content.find(&format!("{}Document", name)) {
                        let def_end = content[def_start..]
                            .find(';')
                            .map(|i| def_start + i + 1)
                            .unwrap_or(def_start + 100);
                        let definition = &content[def_start..def_end.min(def_start + 200)];
                        docs.insert(name.to_string(), definition.to_string());
                    }
                }
            }
        }
    }

    docs
}

const MAX_MEMORY_1000_DOCS: usize = 100 * 1024 * 1024; // 100MB

fn create_1000_file_config(base_dir: &Path) -> graphox::Config {
    use graphox::config::{CodegenConfig, GlobPattern, ProjectConfig, SchemaSource};

    graphox::Config::new_test(
        base_dir.to_owned(),
        vec![
            ProjectConfig::default()
                .with_schema(SchemaSource::Single("schema.graphql".to_string()))
                .with_include(GlobPattern::Single("**/*.graphql".to_string()))
                .with_codegen(CodegenConfig::disabled()),
        ],
    )
    .with_enable_schema_cache(true)
    .with_lsp_automatic_codegen(false)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ntest::timeout(30000)]
#[ignore] // Slow test - loads 1000 documents into memory
async fn test_memory_cached_documents_1000() {
    use std::fs;

    let baseline = crate::support::measure_memory_usage();

    let temp_dir = tempfile::TempDir::new().unwrap();
    let base_dir = temp_dir.path().to_path_buf();

    let schema = crate::support::create_large_schema(100);
    fs::write(base_dir.join("schema.graphql"), schema).unwrap();

    for i in 0..1000 {
        let query = format!("query Query{} {{ item{} {{ id name email }} }}", i, i % 100);
        fs::write(base_dir.join(format!("query_{}.graphql", i)), query).unwrap();
    }

    let config = create_1000_file_config(&base_dir);
    let (mut service, _) =
        tower_lsp::LspService::new(|client| graphox::Backend::new(client, config));
    crate::support::lsp_initialize_sequence(&mut service).await;

    for i in 0..1000 {
        let uri = tower_lsp::lsp_types::Url::from_file_path(
            base_dir.join(format!("query_{}.graphql", i)),
        )
        .unwrap();
        let text = fs::read_to_string(base_dir.join(format!("query_{}.graphql", i))).unwrap();
        crate::support::lsp_did_open(&mut service, uri, "graphql", 1, &text).await;
    }

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let used = crate::support::measure_memory_usage();
    let delta = used.saturating_sub(baseline);

    println!("Memory for 1000 cached documents: {} KB", delta / 1024);
    assert!(
        delta < MAX_MEMORY_1000_DOCS,
        "Memory exceeded limit for 1000 documents: {} KB (limit: {} KB)",
        delta / 1024,
        MAX_MEMORY_1000_DOCS / 1024
    );
}

#[test]
#[ntest::timeout(300000)]
#[ignore] // This test is slow and requires node/npm and wasm32-wasip1 target
fn test_swc_cli_integration() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let root_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture_dir = root_dir.join("tests/fixtures/swc_cli");
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path();

    // Copy fixture files to temp directory
    copy_dir_all_recursive(&fixture_dir, temp_path).expect("Failed to copy fixture files");

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
    assert!(transformed_code.contains("../gen/app.codegen"));
    assert!(!transformed_code.contains("graphql("));
    assert!(!transformed_code.contains("import { graphql }"));
}

fn copy_dir_all_recursive(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all_recursive(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}
