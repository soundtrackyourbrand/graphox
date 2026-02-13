use super::fixtures::run_baseline_test;

#[test]
fn test_emit_extensions_js() {
    run_baseline_test(
        "tests/fixtures/emit_extensions_js",
        "tests/baselines/emit_extensions_js",
        Some("generated"),
    );
}

#[test]
fn test_emit_extensions_ts() {
    run_baseline_test(
        "tests/fixtures/emit_extensions_ts",
        "tests/baselines/emit_extensions_ts",
        Some("generated"),
    );
}

#[test]
fn test_emit_extensions_none() {
    run_baseline_test(
        "tests/fixtures/emit_extensions_none",
        "tests/baselines/emit_extensions_none",
        Some("generated"),
    );
}
