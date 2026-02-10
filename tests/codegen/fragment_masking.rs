use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn get_fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn copy_fixture_to_temp(name: &str) -> (PathBuf, PathBuf) {
    let fixture_path = get_fixture_path(name);
    let temp_dir_obj = tempfile::tempdir().unwrap();
    let temp_dir = temp_dir_obj.path().to_path_buf();

    // We want the directory to persist until the test is over and we manually clean it up
    std::mem::forget(temp_dir_obj);

    if temp_dir.exists() {
        fs::remove_dir_all(&temp_dir).ok();
    }
    fs::create_dir_all(&temp_dir).ok();

    // Copy all files from fixture
    for entry in fs::read_dir(&fixture_path).unwrap() {
        let entry = entry.unwrap();
        let dest = temp_dir.join(entry.file_name());
        fs::copy(entry.path(), &dest).unwrap();
    }

    (temp_dir, fixture_path)
}

#[test]
#[ntest::timeout(300)]
fn test_fragment_masking_enabled() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let (temp_dir, _) = copy_fixture_to_temp("fragment_masking");

    let output = Command::new(bin_path)
        .arg("codegen")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Check that fragment-masking.ts was generated
    let masking_file = temp_dir.join("__generated__").join("fragment-masking.ts");
    assert!(masking_file.exists(), "fragment-masking.ts should exist");

    let masking_content = fs::read_to_string(&masking_file).unwrap();
    assert!(
        masking_content.contains("FragmentType"),
        "fragment-masking.ts should contain FragmentType"
    );
    assert!(
        masking_content.contains("getFragmentData"),
        "fragment-masking.ts should contain getFragmentData"
    );

    // Check that fragments have __fragment property
    let user_fields_file = temp_dir.join("__generated__").join("fragments.codegen.ts");
    assert!(
        user_fields_file.exists(),
        "fragments.codegen.ts should exist"
    );

    let content = fs::read_to_string(&user_fields_file).unwrap();
    assert!(
        content.contains("__fragment:"),
        "Fragment should have __fragment property"
    );
    assert!(
        content.contains("export declare const"),
        "Fragment should have declare const export"
    );

    // Check queries use FragmentType instead of intersection
    let queries_file = temp_dir.join("__generated__").join("queries.codegen.ts");
    assert!(queries_file.exists(), "queries.codegen.ts should exist");

    let queries_content = fs::read_to_string(&queries_file).unwrap();
    assert!(
        queries_content.contains("FragmentType<typeof"),
        "Query should use FragmentType instead of intersection"
    );
    assert!(
        !queries_content.contains("& UserFields"),
        "Query should NOT use intersection type"
    );

    // Check entrypoint imports FragmentType
    let graphql_file = temp_dir.join("__generated__").join("graphql.ts");
    assert!(graphql_file.exists(), "graphql.ts should exist");

    let graphql_content = fs::read_to_string(&graphql_file).unwrap();
    assert!(
        graphql_content.contains("import type { FragmentType }"),
        "graphql.ts should import FragmentType"
    );

    // Cleanup
    fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(300)]
fn test_fragment_masking_disabled() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let (temp_dir, _) = copy_fixture_to_temp("fragment_masking");

    // Modify config to disable fragment masking
    let config_content = r#"
output_dir: "__generated__"

fragmentMasking: disabled

projects:
  - schema: "schema.graphql"
    include: "*.graphql"
"#;
    fs::write(temp_dir.join("graphox.yaml"), config_content).unwrap();

    let output = Command::new(bin_path)
        .arg("codegen")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Check that fragment-masking.ts was NOT generated
    let masking_file = temp_dir.join("__generated__").join("fragment-masking.ts");
    assert!(
        !masking_file.exists(),
        "fragment-masking.ts should NOT exist when disabled"
    );

    // Check queries use intersection type (not FragmentType)
    let queries_file = temp_dir.join("__generated__").join("queries.codegen.ts");
    let queries_content = fs::read_to_string(&queries_file).unwrap();

    assert!(
        !queries_content.contains("FragmentType<typeof"),
        "Query should NOT use FragmentType when disabled"
    );
    assert!(
        queries_content.contains("& UserFields"),
        "Query should use intersection type when disabled"
    );

    // Cleanup
    fs::remove_dir_all(temp_dir).ok();
}

#[test]
#[ntest::timeout(300)]
fn test_fragment_masking_custom_function_name() {
    let bin_path = env!("CARGO_BIN_EXE_graphox");
    let (temp_dir, _) = copy_fixture_to_temp("fragment_masking");

    // Modify config to use custom function name
    let config_content = r#"
output_dir: "__generated__"

fragmentMasking:
  unmaskFunctionName: unmaskFragment

projects:
  - schema: "schema.graphql"
    include: "*.graphql"
"#;
    fs::write(temp_dir.join("graphox.yaml"), config_content).unwrap();

    let output = Command::new(bin_path)
        .arg("codegen")
        .current_dir(&temp_dir)
        .output()
        .expect("Failed to execute process");

    assert!(
        output.status.success(),
        "Codegen failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Check that fragment-masking.ts uses custom function name
    let masking_file = temp_dir.join("__generated__").join("fragment-masking.ts");
    assert!(masking_file.exists(), "fragment-masking.ts should exist");

    let masking_content = fs::read_to_string(&masking_file).unwrap();
    assert!(
        masking_content.contains("unmaskFragment"),
        "fragment-masking.ts should use custom function name"
    );
    assert!(
        !masking_content.contains("getFragmentData"),
        "fragment-masking.ts should NOT contain default function name"
    );

    // Cleanup
    fs::remove_dir_all(temp_dir).ok();
}
