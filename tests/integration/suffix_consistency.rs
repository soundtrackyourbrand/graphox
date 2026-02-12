use super::cli::run_baseline_test;

#[test]
#[ntest::timeout(3000)]
fn test_suffix_consistency() {
    run_baseline_test(
        "tests/fixtures/suffix_consistency",
        "tests/baselines/suffix_consistency",
        Some("__generated__"),
    );
}
