---
name: verify-easyhdr
description: Verify EasyHDR changes with the repository's Windows-native or non-Windows cross-target paths.
---

# Verify EasyHDR

Use this workflow after Rust, Slint, build, or dependency changes. EasyHDR's CI runs on Windows;
the local hooks intentionally substitute cross-target compilation on other hosts.

## 1. Run the universal check

```bash
cargo fmt --all -- --check
```

## 2. Choose the host path

### Windows

Match the main CI job:

```bash
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release --verbose
cargo test --lib --release
cargo test --test integration_tests --release -- --test-threads=1
cargo test --test version_detection_tests --release -- --test-threads=1
cargo test --test memory_usage_test --release -- --test-threads=1
cargo test --test startup_time_test --release -- --test-threads=1
cargo test --test cpu_usage_test --release -- --test-threads=1
```

Keep the integration suites sequential: they exercise Windows API global state.

### Non-Windows

Match `prek.toml`; these checks require `cargo xwin` to already be available:

```bash
cargo xwin clippy --target x86_64-pc-windows-msvc --all-targets --all-features -- -D warnings
cargo xwin check --lib --target x86_64-pc-windows-msvc
```

Do not report this as runtime or Windows-test coverage.

## 3. Narrow feedback while iterating

One library test:

```bash
cargo test --lib config::manager::tests::test_load_missing_config -- --exact
```

One integration test:

```bash
cargo test --test version_detection_tests test_non_windows_platform -- --exact
```

If a library test reads or writes configuration, use `AppdataGuard` from `src/test_utils.rs`;
do not mutate `APPDATA` directly. For Windows API integration behavior, rerun the containing
suite with `--test-threads=1` before considering it verified.
