use std::fs;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::tempdir;

#[test]
#[ntest::timeout(5000)]
fn test_codegen_watch_mode() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
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

    let config_path = base_dir.join("graphql.yaml");
    fs::write(
        &config_path,
        "projects:\n  - schema: \"schema.graphql\"\n    include: \"query.graphql\"",
    )
    .unwrap();

    // 2. Spawn codegen in watch mode
    let mut child = Command::new(bin_path)
        .arg("codegen")
        .arg("--watch")
        .current_dir(base_dir)
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to spawn codegen watcher");
    // Ensure we wait on the child in case of early return to avoid zombies
    let _child_pid = child.id();

    let gen_file = base_dir.join("query.codegen.ts");

    // 3. Wait for initial generation
    if !wait_for_file(&gen_file, Duration::from_secs(2)) {
        child.kill().ok();
        panic!("Initial codegen file not created in time");
    }

    let initial_content = fs::read_to_string(&gen_file).unwrap();
    assert!(initial_content.contains("me: {"));
    assert!(!initial_content.contains("name: string"));

    // 4. Modify query to include 'name'
    fs::write(&query_path, "query GetMe { me { id name } }").unwrap();

    // 5. Wait for updated generation
    let mut updated = false;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if let Ok(content) = fs::read_to_string(&gen_file)
            && content.contains("name: string | null")
        {
            updated = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let _ = child.kill();
    let _ = child.wait();

    assert!(
        updated,
        "Codegen file was not updated after change in watch mode"
    );
}

#[test]
#[ntest::timeout(5000)]
fn test_codegen_watch_schema_changes() {
    let bin_path = env!("CARGO_BIN_EXE_graphql-rust");
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

    let config_path = base_dir.join("graphql.yaml");
    fs::write(
        &config_path,
        "projects:\n  - schema: \"schema.graphql\"\n    include: \"query.graphql\"",
    )
    .unwrap();

    // 2. Spawn codegen in watch mode
    let mut child = Command::new(bin_path)
        .arg("codegen")
        .arg("--watch")
        .current_dir(base_dir)
        .spawn()
        .expect("Failed to spawn codegen watcher");
    let _child_pid2 = child.id();

    let gen_file = base_dir.join("query.codegen.ts");

    // 3. Wait for initial generation
    if !wait_for_file(&gen_file, Duration::from_secs(2)) {
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
    std::thread::sleep(Duration::from_millis(50));

    // 5. Modify query to use the new field
    fs::write(&query_path, "query GetMe { me { id email } }").unwrap();

    // 6. Wait for updated generation
    let mut updated = false;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        if let Ok(content) = fs::read_to_string(&gen_file)
            && content.contains("email: string")
        {
            updated = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let _ = child.kill();
    let _ = child.wait();

    assert!(
        updated,
        "Codegen file was not updated after schema + query change in watch mode"
    );
}

fn wait_for_file(path: &std::path::Path, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}
