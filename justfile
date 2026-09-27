# Invoke heavy recipes through the host's run-heavy wrapper.
check:
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
    cargo test --all-targets -- --test-threads=1

regressions:
    cargo test compile::regression_tests -- --test-threads=1
    cargo test version::tests -- --test-threads=1

baseline-wip:
    cargo build --release --locked
    cargo test --test default_baseline_wip_test -- --ignored --nocapture
