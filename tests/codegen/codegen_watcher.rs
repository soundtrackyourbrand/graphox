use std::fs;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tempfile::tempdir;

#[test]
#[ntest::timeout(30000)]
fn test_codegen_watch_mode() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    // 1. Setup schema and query
    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { me: User } type User { id: ID! name: String }",
    )
    .unwrap();

    let query_path = base_dir.join("query.graphql");
    fs::write(&query_path, "query GetMe { me { id } }").unwrap();

    let config_path = base_dir.join("graphox.yaml");
    fs::write(
        &config_path,
        "enable_schema_cache: false\ncodegen_watch_debounce_ms: 10\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"query.graphql\"\n    output_dir: \"gen\"",
    )
    .unwrap();

    // 2. Spawn codegen in watch mode
    let mut child = Command::new(bin_path)
        .arg("codegen")
        .arg("--watch")
        .arg("--verbose")
        .current_dir(base_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to spawn codegen watcher");

    let gen_file = base_dir.join("gen/query.codegen.ts");

    // 3. Wait for initial generation
    if !wait_for_file(&gen_file, Duration::from_secs(10)) {
        child.kill().ok();
        panic!("Initial codegen file not created in time");
    }

    let initial_content = fs::read_to_string(&gen_file).unwrap();
    assert!(initial_content.contains("me: {"));
    assert!(!initial_content.contains("name: string"));

    // Give watcher time to settle
    thread::sleep(Duration::from_millis(200));

    // 4. Modify query to include 'name'
    fs::write(&query_path, "query GetMe { me { id name } }").unwrap();

    // 5. Wait for updated generation
    let mut updated = false;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        if let Ok(content) = fs::read_to_string(&gen_file)
            && content.contains("name: string | null")
        {
            updated = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    let _ = child.kill();
    let _ = child.wait();

    assert!(
        updated,
        "Codegen file was not updated after change in watch mode"
    );
}

#[test]
#[ntest::timeout(3000)]
fn test_codegen_watch_schema_changes() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let dir = tempdir().unwrap();
    let base_dir = dir.path();

    // 1. Setup schema and query
    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { me: User } type User { id: ID! }",
    )
    .unwrap();

    let query_path = base_dir.join("query.graphql");
    fs::write(&query_path, "query GetMe { me { id } }").unwrap();

    let config_path = base_dir.join("graphox.yaml");
    fs::write(
        &config_path,
        "enable_schema_cache: false\ncodegen_watch_debounce_ms: 10\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"query.graphql\"\n    output_dir: \"gen\"",
    )
    .unwrap();

    // 2. Spawn codegen in watch mode
    let mut child = Command::new(bin_path)
        .arg("codegen")
        .arg("--watch")
        .arg("--verbose")
        .current_dir(base_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start watch mode");

    let gen_file = base_dir.join("gen/query.codegen.ts");

    // 3. Wait for initial generation
    if !wait_for_file(&gen_file, Duration::from_secs(10)) {
        child.kill().ok();
        panic!("Initial codegen file not created in time");
    }

    // 4. Modify schema to add 'email' field to User
    fs::write(
        &schema_path,
        "type Query { me: User } type User { id: ID! email: String! }",
    )
    .unwrap();

    // Give it a moment to detect schema change and re-evaluate
    thread::sleep(Duration::from_millis(200));

    // 5. Modify query to use the new field
    fs::write(&query_path, "query GetMe { me { id email } }").unwrap();

    // 6. Wait for updated generation
    let mut updated = false;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        if let Ok(content) = fs::read_to_string(&gen_file)
            && content.contains("email: string")
        {
            updated = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }

    let _ = child.kill();
    let _ = child.wait();

    assert!(
        updated,
        "Codegen file was not updated after schema + query change in watch mode"
    );
}

#[test]
#[ntest::timeout(3000)]
fn test_codegen_idempotent_writes() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let dir = tempdir().unwrap();
    let base_dir = fs::canonicalize(dir.path()).unwrap();

    // 1. Setup schema and config with schema_types
    let schema_path = base_dir.join("schema.graphql");
    fs::write(&schema_path, "type Query { me: String }").unwrap();

    let output_path = base_dir.join("generated-schema.ts");

    let config_path = base_dir.join("graphox.yaml");
    fs::write(
        &config_path,
        "enable_schema_cache: false\nschema_types:\n  - schema: \"schema.graphql\"\n    output: \"generated-schema.ts\"",
    )
    .unwrap();

    // 2. Run codegen first time
    let status = Command::new(bin_path)
        .arg("codegen")
        .current_dir(&base_dir)
        .status()
        .expect("Failed to run codegen");
    assert!(status.success());

    assert!(output_path.exists());
    let initial_mtime = fs::metadata(&output_path).unwrap().modified().unwrap();

    // Give some time for mtime to be different if it were rewritten
    thread::sleep(Duration::from_millis(100));

    // 3. Run codegen second time without changes
    let status = Command::new(bin_path)
        .arg("codegen")
        .current_dir(&base_dir)
        .status()
        .expect("Failed to run codegen second time");
    assert!(status.success());

    let second_mtime = fs::metadata(&output_path).unwrap().modified().unwrap();
    assert_eq!(
        initial_mtime, second_mtime,
        "Output file should not be rewritten if content is identical"
    );

    // 4. Modify the output file manually (simulating formatter)
    let formatted_content =
        "// formatted\n".to_string() + &fs::read_to_string(&output_path).unwrap();
    fs::write(&output_path, &formatted_content).unwrap();
    let after_format_mtime = fs::metadata(&output_path).unwrap().modified().unwrap();

    thread::sleep(Duration::from_millis(100));

    // 5. Run codegen third time - it SHOULD overwrite the formatted content because it differs
    let status = Command::new(bin_path)
        .arg("codegen")
        .current_dir(&base_dir)
        .status()
        .expect("Failed to run codegen third time");
    assert!(status.success());

    let final_mtime = fs::metadata(&output_path).unwrap().modified().unwrap();
    assert_ne!(
        after_format_mtime, final_mtime,
        "Output file should be rewritten if content differs (even if just formatting)"
    );

    let final_content = fs::read_to_string(&output_path).unwrap();
    assert!(
        !final_content.starts_with("// formatted"),
        "Graphox should have restored its own content"
    );

    // 6. Run codegen fourth time - it should NOT overwrite now that content is back to normal
    thread::sleep(Duration::from_millis(100));
    let status = Command::new(bin_path)
        .arg("codegen")
        .current_dir(&base_dir)
        .status()
        .expect("Failed to run codegen fourth time");
    assert!(status.success());
    let final_stable_mtime = fs::metadata(&output_path).unwrap().modified().unwrap();
    assert_eq!(
        final_mtime, final_stable_mtime,
        "Output file should be stable after restoration"
    );
}

#[test]
#[ntest::timeout(30000)]
fn test_codegen_watch_ignores_generated_files() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let dir = tempdir().unwrap();
    let base_dir = fs::canonicalize(dir.path()).unwrap();

    // 1. Setup schema and config with schema_types
    let schema_path = base_dir.join("schema.graphql");
    fs::write(&schema_path, "type Query { me: String }").unwrap();

    let output_path = base_dir.join("generated-schema.ts");

    let config_path = base_dir.join("graphox.yaml");
    fs::write(
        &config_path,
        "enable_schema_cache: false\ncodegen_watch_debounce_ms: 10\nschema_types:\n  - schema: \"schema.graphql\"\n    output: \"generated-schema.ts\"",
    )
    .unwrap();

    // 2. Spawn codegen in watch mode
    let mut child = Command::new(bin_path)
        .arg("codegen")
        .arg("--watch")
        .arg("--verbose")
        .current_dir(&base_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("Failed to spawn codegen watcher");

    // Wait for initial generation
    let mut count = 0;
    while !output_path.exists() && count < 100 {
        thread::sleep(Duration::from_millis(50));
        count += 1;
    }
    assert!(output_path.exists(), "Initial output not generated");

    // Give watcher time to settle
    thread::sleep(Duration::from_millis(100));

    // 4. Modify the output file manually (simulating formatter)
    // This modification should be IGNORED by the watcher.
    fs::write(
        &output_path,
        "// modified by formatter\nexport type Query = { me: string };",
    )
    .unwrap();

    // 5. Wait for a while to see if Graphox overwrites it
    // If the watcher is working correctly (ignoring the file), it won't trigger codegen.
    thread::sleep(Duration::from_millis(300));

    let current_content = fs::read_to_string(&output_path).unwrap();

    let _ = child.kill();
    let _ = child.wait();

    assert!(
        current_content.starts_with("// modified"),
        "Graphox should NOT have overwritten the file because the watcher should have ignored the change"
    );
}

#[test]
fn test_is_output_file_recognition() {
    use graphox::Config;
    let dir = tempdir().unwrap();
    let base_dir = fs::canonicalize(dir.path()).unwrap();

    let config_text = r#"
projects:
  - include: "src"
    output_dir: "gen"
    schema: "schema.graphql"
schema_types:
  - schema: "schema.graphql"
    output: "types/schema.ts"
    possible_types: "types/possible.ts"
"#;
    fs::write(base_dir.join("graphox.yaml"), config_text).unwrap();
    let config = Config::load_from_dir(&base_dir).unwrap().unwrap();

    // Test project output dir
    assert!(config.is_output_file(&base_dir.join("gen/file.ts")));
    assert!(config.is_output_file(&base_dir.join("gen/sub/file.ts")));

    // Test schema_types output
    assert!(config.is_output_file(&base_dir.join("types/schema.ts")));
    assert!(config.is_output_file(&base_dir.join("types/possible.ts")));

    // Test source files (should NOT be identified as output)
    assert!(!config.is_output_file(&base_dir.join("src/query.ts")));
    assert!(!config.is_output_file(&base_dir.join("schema.graphql")));
}

#[test]
#[ntest::timeout(30000)]
fn test_codegen_watch_ignores_non_graphql_host_edits() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let dir = tempdir().unwrap();
    let base_dir = fs::canonicalize(dir.path()).unwrap();

    let schema_path = base_dir.join("schema.graphql");
    fs::write(
        &schema_path,
        "type Query { me: User } type User { id: ID! name: String }",
    )
    .unwrap();
    let query_path = base_dir.join("query.graphql");
    fs::write(&query_path, "query GetMe { me { id } }").unwrap();
    let plain_path = base_dir.join("plain.ts");
    fs::write(&plain_path, "export const value = 1;\n").unwrap();

    let config_path = base_dir.join("graphox.yaml");
    fs::write(
        &config_path,
        "enable_schema_cache: false\ncodegen_watch_debounce_ms: 10\nprojects:\n  - schema: \"schema.graphql\"\n    include: \"query.graphql\"\n    output_dir: \"gen\"",
    )
    .unwrap();

    let mut child = Command::new(bin_path)
        .arg("codegen")
        .arg("--watch")
        .arg("--verbose")
        .current_dir(&base_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to spawn codegen watcher");

    let gen_file = base_dir.join("gen/query.codegen.ts");
    if !wait_for_file(&gen_file, Duration::from_secs(10)) {
        child.kill().ok();
        panic!("Initial codegen file not created in time");
    }

    fs::write(
        &gen_file,
        "// touched by formatter\nexport const untouched = true;\n",
    )
    .unwrap();
    thread::sleep(Duration::from_millis(200));

    fs::write(&plain_path, "export const value = 2;\n").unwrap();
    thread::sleep(Duration::from_millis(400));

    let content = fs::read_to_string(&gen_file).unwrap();

    let _ = child.kill();
    let _ = child.wait();

    assert!(
        content.starts_with("// touched by formatter"),
        "Non-GraphQL host edit should not trigger watch-mode codegen"
    );
}

fn wait_for_file(path: &std::path::Path, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}
