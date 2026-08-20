# AGENTS.md

This file provides guidance to AI coding agents when working with code in this
repository.

EasyHDR — a Windows-only desktop app (Rust 2024 edition, `rust-version = 1.93`) that toggles
display HDR when configured applications run. Slint GUI, threads + channels, no async runtime.

## Platform constraint

The real target is `x86_64-pc-windows-msvc`. The tree compiles on non-Windows hosts only
because every Windows-touching module carries a `#[cfg(not(windows))]` stub — 58 of them across
`src/`. When you add Windows FFI, add the non-Windows arm too, or host builds break.

`prek.toml` branches on the host, so the checks you run locally depend on your OS:

| | Windows host | Other host |
| --- | --- | --- |
| Lint | `cargo clippy --all-targets --all-features -- -D warnings` | `cargo xwin clippy --target x86_64-pc-windows-msvc --all-targets --all-features -- -D warnings` |
| Test | `cargo test --lib` | `cargo xwin check --lib --target x86_64-pc-windows-msvc` (compile only) |

Everything under `tests/` gated on `#[cfg(windows)]` — `cpu_profiling_test`,
`uwp_process_detection_tests`, most of `cpu_usage_test` — is unrunnable off Windows. A green
run on macOS/Linux is not coverage of those paths.

## Commands

CI (`.github/workflows/ci.yml`, `windows-latest`) runs, in order:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release --verbose
cargo test --lib --release
```

Integration suites run one file at a time, single-threaded — Windows API global state:

```bash
cargo test --test integration_tests --release -- --test-threads=1
# repeat for: version_detection_tests memory_usage_test startup_time_test cpu_usage_test
```

`icon_cache_tests` and `uwp_process_detection_tests` also need `--test-threads=1` but are **not
in CI** — run them by hand after touching icon caching or UWP detection.
`uwp_process_detection_tests` additionally requires Microsoft.WindowsCalculator installed.

Single test case:

```bash
cargo test --lib config::manager::tests::test_config_path       # unit test inside src/
cargo test --test integration_tests -- --exact test_config_persistence_integration --test-threads=1
```

`cargo bench` needs no extra flags — `bench = false` on `[lib]` and `[[bin]]` keeps libtest
from swallowing Criterion's arguments.

## Lint gates

CI is `-D warnings` over `clippy::pedantic`, plus `unsafe_code = "warn"` and
`missing_docs = "warn"` (see `[lints]` in `Cargo.toml`). Consequences:

- Every `pub` item needs a doc comment — including new enum variants and struct fields.
- Every `unsafe` block needs `#[expect(unsafe_code, reason = "...")]` on the enclosing item and
  a `// SAFETY:` comment inside.
- Suppress lints with `#[expect(lint, reason = "...")]`, not `#[allow(...)]` — the tree has 118
  of the former against 1 of the latter, and `expect` fails the build once the suppression goes
  stale.

## Layout invariants

- `src/gui/` is **binary-only** (`mod gui;` in `src/main.rs`); it is not declared in
  `src/lib.rs`. Integration tests under `tests/` cannot reach it — GUI logic that needs test
  coverage belongs in `src/controller/` or `src/utils/`.
- `slint::include_modules!()` appears only in `src/main.rs`, so Slint types are unavailable to
  the library and to `tests/`.
- `src/uwp/` is entirely `#[cfg(windows)]`, down to the `mod` declarations in its `mod.rs`.

Runtime topology, wired in `initialize_components` (`src/main.rs`): `ProcessMonitor` and
`HdrStateMonitor` each own a thread and push into bounded `mpsc::sync_channel`s (capacity 32);
`AppController` owns the event loop and the `HdrController`; `GuiController` holds an
`Arc<Mutex<AppController>>` and drains an `AppState` channel.

## Footguns

**Adding a user setting touches five places.** Miss one and it silently no-ops:

1. `ui/main.slint` — property + widget on `SettingsDialogContent`
2. `ui/main.slint` — mirrored `settings-*` property on `MainWindow`, plus its `<=>` alias inside
   the `SettingsDialogContent { }` block
3. `ui/main.slint` — the `save-settings(...)` callback signature, declared in *both* components
4. `src/config/models.rs` — field on `UserPreferences`, marked `#[serde(default)]`
5. `src/gui/gui_controller.rs` — the `set_settings_*` call on load and the `on_save_settings`
   closure arity

**Config back-compat is hand-written.** `MonitoredApp` has a manual `Deserialize`
(`src/config/models.rs`) that accepts both `app_type`-tagged entries and an untagged legacy
Win32 shape. New `Win32App` / `UwpApp` fields need `#[serde(default)]`, or existing
`%APPDATA%\EasyHDR\config.json` files stop loading.

**Advisory ignores live in two files.** `deny.toml` (for `cargo deny`) and `.cargo/audit.toml`
(for `cargo audit`) keep separate `ignore` lists, and `.github/workflows/security.yml` runs
both — add a RUSTSEC id to both, with the justification comment the existing entries use.

**Tests that touch config must take an `AppdataGuard`** from `src/test_utils.rs`
(`create_test_dir()` then `AppdataGuard::new(&dir)`). It holds a process-wide mutex and
redirects `%APPDATA%`; without it, a test reads and writes the developer's real config.

**HDR control forks on `WindowsVersion`** at three `match self.windows_version` sites in
`src/hdr/controller.rs` (`Windows10 | Windows11` share a path, `Windows11_24H2` differs). A new
branch has to be added at all three.

## Generated / derived

- `build.rs` compiles `ui/main.slint` into `OUT_DIR`. Edit the `.slint` file, never the
  generated Rust.
- `build.rs` also bakes in `GIT_COMMIT_SHA` and `CARGO_PKG_VERSION`, read via `env!` in
  `src/main.rs` and `src/gui/gui_controller.rs`.
- Release: bump `version` in `Cargo.toml`, then publish a GitHub Release —
  `.github/workflows/release.yml` fires on the `published` event and uploads `easyhdr.exe`.

## Commits

`prek` (config in `prek.toml`) enforces Conventional Commits and blocks direct commits to
`main` / `master`. Hooks are not installed in a fresh clone; run `prek install`.

## Reference

- `.agents/rules/rust-dev-guidelines.md` — 763-line Rust 2024 / 1.93 feature and idiom
  reference, marked `agent_requested`. Read on demand when reaching for a modern-Rust API you
  are unsure is stable; it is not background reading.
- `README.md` — end-user install, Criterion performance baselines, and the
  `cargo +nightly fuzz run` invocations for the three targets under `fuzz/`.
