# CI and dependency maintenance

All PRs, default-branch pushes and exact-commit repair dispatches run the same
required gate. `ci / required` requires the dispatch guard, native Windows build
and tests, security workflow, portable Miri workflow and repository hygiene.
Missing, skipped, cancelled or failed required jobs block merging. Renovate updates
merge unattended only after all current-head checks in `.github/merge-policy.json`
succeed. For other changes, review the exact
head and base, full diff, authorship, all expected CI jobs and native artifacts
before merging with the maintainer's `ghmerge` function. Repository protections and
rulesets are intentionally disabled. Actions
and shared workflows use full release tags.

## Native validation and local parity

`rust-toolchain.toml` pins Rust 1.98.0 with rustfmt and Clippy. The manifest's 1.93
minimum remains unchanged; this pipeline validates the pinned toolchain rather than
claiming an independent minimum-version build. Windows 2025 runs:

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo build --release --locked --verbose
cargo test --locked --lib --release
cargo test --locked --test integration_tests --release -- --test-threads=1
cargo test --locked --test version_detection_tests --release -- --test-threads=1
cargo test --locked --test memory_usage_test --release -- --test-threads=1
cargo test --locked --test startup_time_test --release -- --test-threads=1
cargo test --locked --test cpu_usage_test --release -- --test-threads=1
cargo test --locked --test icon_cache_tests --release -- --test-threads=1
cargo test --locked --doc --release
```

The existing portable `-C target-cpu=x86-64` release baseline is retained. Bash
ensures an early failing Cargo invocation cannot be hidden by a later successful
command. The previously omitted icon-cache suite is now required. The tested
executable is uploaded; tracked-file mutation fails validation. Local prek keeps
its native-Windows versus cargo-xwin behavior and now uses locked resolution.
The separate hygiene job skips duplicate Cargo checks.

Linux stubs are not Windows coverage. Calculator-dependent UWP start/stop tests,
real HDR displays, UI interaction and older Windows versions still need a suitable
Windows desktop test environment. The existing UWP test file includes an ignored
stop test and early returns when applications cannot launch; it is not treated as
a required unattended test suite. Profiling remains a separate workload with its
existing path triggers and manual controls.

## Security, Miri and dependency updates

The reusable security workflow requires versioned cargo-audit and cargo-deny tools.
The existing advisory exceptions in `deny.toml` and `.cargo/audit.toml` remain
explicit; this rollout does not add new advisory ignores. Security checks also run
daily. Informational cargo-geiger runs on that schedule and remains outside the
required security checks. The portable Miri error-module test is mandatory using
`nightly-2026-09-07`; its existing symbolic-alignment/isolation flags are retained.
Broader best-effort Miri runs stay weekly because their dependency/FFI limitations
are already documented in the workflow. The nightly date needs manual maintenance
when newer Miri/compiler behavior is adopted.

The shared Renovate preset retains non-major grouping, manages Cargo, actions,
hooks and the stable toolchain, and keeps immutable shared workflow references
current through PRs. The v1.1.0 default and automerge presets make all dependency
update types eligible, including Cargo majors and shared-policy updates, without
dashboard approval. The checked merge preserves genuine sign-offs and dispatches
the full CI workflow for the exact merged commit. The existing renderer, hardware
and UWP coverage gaps remain documented; no tests or advisory gates are weakened.

Release publishing waits for the full development pipeline. VirusTotal uses release
credentials only in trusted release jobs; optional default-branch scans require
`HAS_VT_KEY=true`. PR validation never receives the VirusTotal key. Existing release
notes and profiling artifacts remain available. Native execution, security-database
results and Miri compatibility must pass GitHub CI before this adoption is complete.
