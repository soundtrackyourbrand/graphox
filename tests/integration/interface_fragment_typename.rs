use super::fixtures::run_baseline_test;

#[test]
#[ntest::timeout(1000)]
fn test_interface_fragment_typename() {
    run_baseline_test(
        "tests/fixtures/interface_fragment_typename",
        "tests/baselines/interface_fragment_typename",
        None,
    );
}
