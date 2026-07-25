# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this
repository.

## Scope and toolchain

- EasyHDR ships only for Windows 10 build 19044+; non-Windows implementations are compile/test
  stubs, not a second supported runtime (`src/main.rs`, `.github/workflows/ci.yml`).
- Use Rust 2024 with the `rust-version = "1.93"` floor from `Cargo.toml`.
- The authoritative UI source is `ui/main.slint`. `build.rs` compiles it for
  `slint::include_modules!()` in `src/main.rs`; change the Slint source rather than generated
  Rust output.

## Boundaries that are easy to get wrong

- `src/gui/` belongs to the binary because it consumes Slint-generated types. Keep reusable and
  testable application logic in the library modules exported by `src/lib.rs`.
- `AppController` owns the shared configuration and synchronizes it with `ProcessMonitor` watch
  state. Route application/configuration mutations through it rather than updating GUI state or
  monitor state independently.
- Preserve the configuration compatibility logic in `src/config/models.rs`: legacy untagged
  applications migrate to Win32 entries, malformed individual entries are skipped without
  discarding valid entries, and `icon_data` is intentionally absent from JSON.
- Icons are derived data under `%APPDATA%\EasyHDR\icon_cache`; update cache/extraction behavior
  instead of adding icon bytes to `%APPDATA%\EasyHDR\config.json`.
- Library tests that alter `APPDATA` must use `crate::test_utils::AppdataGuard`. Windows
  integration suites run sequentially because they touch process/display global state.

## Verification

- Format: `cargo fmt --all -- --check`
- Native Windows CI: `cargo clippy --all-targets --all-features -- -D warnings`, then
  `cargo build --release --verbose`, `cargo test --lib --release`, and the sequential integration
  suites in `.github/workflows/ci.yml`.
- On non-Windows hosts, the configured hooks use `cargo xwin` for MSVC-target clippy/check and do
  not execute the Windows tests. Read `.claude/skills/verify-easyhdr/SKILL.md` before choosing a
  verification path or running a single test.

## Reference rules

- `.augment/rules/rust-dev-guidelines.md` — repository-selected Rust 1.93+/2024-edition guidance.
  Read before changing Rust code; do not duplicate it here.
