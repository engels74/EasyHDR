# CLAUDE.md

This file provides guidance to AI coding agents when working in this repository.

## Project overview

EasyHDR is a Windows-only Rust application that automatically toggles HDR on configured displays when monitored apps start and stop. Single crate (binary + library):

- Binary entry point: `src/main.rs` (wires startup, owns the `gui` module).
- Library entry point: `src/lib.rs` exposes `config`, `controller`, `hdr`, `monitor`, `utils`, and `uwp` (Windows-only).
- UI: declarative Slint in `ui/main.slint`, compiled by `build.rs` and pulled in via `slint::include_modules!()` in `main.rs`.
- The `gui` module (`src/gui/`) lives in the binary only — not in the library — because it owns the generated `MainWindow` type.

Common edit locations:

- HDR enable/disable + Windows Display Config FFI → `src/hdr/`.
- Process polling and UWP detection → `src/monitor/process_monitor.rs`, `src/uwp/`.
- Coordination, debouncing, state events → `src/controller/app_controller.rs`.
- Persistence and models → `src/config/` (config lives at `%APPDATA%\EasyHDR\config.json`).
- Icon cache, autostart, single-instance, logging, update checker, profilers → `src/utils/`.
- UI bindings, tray icon → `src/gui/`.
- Slint UI → `ui/main.slint`.

Target: Windows 10 21H2+ (`MIN_WINDOWS_BUILD = 19044` in `src/main.rs`). On non-Windows platforms the binary still compiles for development convenience but exits early with a message; most modules are gated by `#[cfg(windows)]`.

## Commands

Run from the repo root. Rust 2024 edition, MSRV 1.93 (see `Cargo.toml`).

```bash
# Build
cargo build                       # dev (opt-level=1, line-tables-only debug)
cargo build --release             # LTO, codegen-units=1, strip, panic=abort

# Format / lint / docs (these are what CI enforces)
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings

# Unit tests (parallel is fine — library tests are pure Rust)
cargo test --lib --release

# Integration tests — MUST be sequential, Windows API has global state
cargo test --test integration_tests       --release -- --test-threads=1
cargo test --test version_detection_tests --release -- --test-threads=1
cargo test --test memory_usage_test       --release -- --test-threads=1
cargo test --test startup_time_test       --release -- --test-threads=1
cargo test --test cpu_usage_test          --release -- --test-threads=1

# Single test case
cargo test --lib --release some_test_name -- --exact
cargo test --test integration_tests --release some_test_name -- --test-threads=1 --exact

# Benchmarks (Criterion; bench=false in [lib]/[bin] to allow Criterion args)
cargo bench
cargo bench --bench config         # or: hdr_detection | icon_cache | process_monitor_bench | uwp_detection

# Profiling profile (frame pointers, full debug, opt-level=3, thin LTO)
RUSTFLAGS="-C force-frame-pointers=yes" cargo build --profile profiling
```

CI (`.github/workflows/ci.yml`) runs the four commands in the first three sections on `windows-latest`. To approximate CI locally, run fmt-check, clippy, the release build, then unit + sequential integration tests in that order.

`.pre-commit-config.yaml` enforces `cargo fmt --check` and `cargo clippy -D warnings` pre-commit, and `cargo test --lib` pre-push. On non-Windows hosts the pre-commit hooks fall back to `cargo xwin` against `x86_64-pc-windows-msvc`.

## High-level architecture

Multi-threaded, event-driven, channels between components:

1. **Main thread** — Slint event loop (`gui::GuiController::run` → `slint::run_event_loop_until_quit()`). Stays alive when the window is hidden so the tray keeps working.
2. **ProcessMonitor thread** (`src/monitor/process_monitor.rs`) — polls processes via Toolhelp32 (`Win32_System_Diagnostics_ToolHelp`), matches by exe filename or UWP package family name, and sends `ProcessEvent` over a `std::sync::mpsc::SyncSender`. Watch list lives in a shared `Arc<RwLock<WatchState>>` so the controller and monitor stay in sync without races.
3. **AppController thread** (`src/controller/app_controller.rs`) — owns the `HdrController`, debounces toggles (~500ms via atomic nanosecond timestamps), consumes `ProcessEvent` + `HdrStateEvent`, publishes `AppState` snapshots to the GUI through another `SyncSender`. Spawned with `AppController::spawn_event_loop(Arc<Mutex<AppController>>)` — the lock is taken per event so GUI callbacks aren't blocked.
4. **HdrStateMonitor thread** (`src/monitor/hdr_state_monitor.rs`) — hidden Win32 window receiving `WM_DISPLAYCHANGE` / `WM_SETTINGCHANGE`, with periodic re-checks (500ms × up to 10) because the Display Config APIs lag the broadcast messages. Detects external HDR toggles so the UI stays in sync.

`HdrController` (`src/hdr/controller.rs`) calls Windows Display Configuration APIs. Different code paths for Windows 10, Windows 11, and Windows 11 24H2+ are dispatched in `src/hdr/version.rs`; raw FFI structs/declarations are in `src/hdr/windows_api.rs`.

UWP/AppX support (`src/uwp/`) is Windows-only and uses WinRT `Management.Deployment.PackageManager`, with icons extracted via `Package.GetLogo()` and cached as PNG.

`build.rs` does three things: compile Slint UI, embed `CARGO_PKG_VERSION` + short git SHA via `cargo:rustc-env`, and on Windows embed icon/version-info/manifest via `winres`.

## Task workflows

### Adding a new monitored-app type or process detection rule

1. Extend models in `src/config/models.rs` (data) — check existing `MonitoredApp` / `Win32App` shapes.
2. Update matching logic in `src/monitor/process_monitor.rs` (and `src/uwp/detector.rs` if UWP-relevant).
3. If matching state must be shared, update `WatchState` and route through `Arc<RwLock<WatchState>>` rather than adding a new lock.
4. Add a unit test in the same module and an integration test under `tests/` if it crosses module boundaries.

### Adding an HDR API code path (new Windows build)

1. Add the gating helper in `src/hdr/version.rs`.
2. Implement the API call in `src/hdr/controller.rs`, reusing the FFI bindings in `src/hdr/windows_api.rs` rather than re-declaring `extern` blocks.
3. Add a sequential test in `tests/version_detection_tests.rs`.

### Adding a UI control

1. Edit `ui/main.slint` (`MainWindow` is exported at the bottom). Don't bypass `build.rs` — it regenerates the Rust bindings.
2. Wire callbacks and properties in `src/gui/gui_controller.rs`. State that needs to round-trip through the controller flows as `AppState` snapshots, not direct mutations.

### Adding configuration

1. Add fields to `src/config/models.rs` with `serde` defaults so older configs still deserialize.
2. Read/write through `ConfigManager` in `src/config/manager.rs` — it owns the `%APPDATA%\EasyHDR\config.json` path and atomic-write semantics. Don't open the file directly.

### Adding a benchmark

Add a file under `benches/`, register it in `Cargo.toml` as `[[bench]] name = "...", harness = false`. Existing benches all use Criterion; `[lib]` and `[bin]` have `bench = false` so Criterion CLI args (e.g. `--save-baseline`) work.

## Decision tables

| Situation | Use this | Avoid |
| --- | --- | --- |
| Locking shared state | `parking_lot::{Mutex, RwLock}` | `std::sync::{Mutex, RwLock}` |
| Cross-thread events | `std::sync::mpsc::SyncSender` matching existing channels | Spawning ad-hoc threads with new channels |
| Error type for fallible function | `easyhdr::Result<T>` (alias) + variants of `EasyHdrError` in `src/error.rs`, preserving the source with `#[source]` or `#[from]` | `anyhow::Error` inside the library (`anyhow` is only used in `main.rs` for top-level startup context) |
| Logging | `tracing::{info, warn, error, debug}` | `println!` / `eprintln!` (only acceptable in the non-Windows fallback in `main.rs`) |
| User-facing error message | Add a branch in `get_user_friendly_error` (`src/error.rs`) | Inline strings in the GUI |
| File dialog / message box | `rfd` | Hand-rolled `MessageBoxW` |
| HTTP (update check) | `reqwest` blocking client (already a dep) | Adding async runtime + async client |
| Persisting bytes to disk | Atomic write pattern from `utils/icon_cache.rs` (`tempfile` + rename) | `std::fs::write` directly for user-visible files |
| Long-running CPU-parallel work | `rayon` (already used for icon cache loading) | Manual thread pools |
| Adding `unsafe` | Wrap minimal block in `unsafe { ... }` inside a normal `fn`, add `// SAFETY:` doc and `#[expect(unsafe_code, reason = "...")]` | Marking whole modules with `#![allow(unsafe_code)]` |

## Code patterns

Error variants preserve the source so chains are inspectable (`src/error.rs`):

```rust
#[error("Failed to control HDR: {0}")]
HdrControlFailed(#[source] Box<dyn std::error::Error + Send + Sync>),
```

Windows-only code is feature-gated, with a non-Windows stub or `unused_variables` `expect` for the dev compile path (`src/gui/tray.rs`, `src/main.rs`):

```rust
#[cfg(windows)]
pub struct TrayIcon { /* ... */ }

#[cfg(not(windows))]
pub struct TrayIcon;
```

Shared state lives behind `Arc<parking_lot::*>`; the event loop takes the controller lock only while handling a single event (`src/controller/app_controller.rs`):

```rust
pub fn spawn_event_loop(controller: Arc<Mutex<AppController>>) -> JoinHandle<()> {
    // ... take receivers under one lock, then release ...
    std::thread::spawn(move || { /* per-event lock */ })
}
```

Lints to expect (`Cargo.toml [lints]`): `unsafe_code = "warn"`, `missing_docs = "warn"`, `clippy::pedantic` and `clippy::all` at warn, `clippy::unwrap_used = "warn"`. New public items need a `///` doc comment; new `unsafe` blocks need a `SAFETY:` comment.

## Project-specific rules

- Run integration tests under `tests/` with `--test-threads=1`; they manipulate HDR/display state on the host.
- Don't add `gui` types or Slint generated items to the library crate — `MainWindow` is only available in the binary because `slint::include_modules!()` runs in `src/main.rs`.
- `windows-rs` is on the version pinned in `Cargo.toml` and doesn't expose every Display Config API needed; manually declared FFI lives in `src/hdr/windows_api.rs` — extend that file instead of redeclaring `extern "system"` blocks elsewhere.
- `anyhow` is only for the top-level binary startup in `main.rs` (`Context`/`Result`). Library code returns `easyhdr::Result<T>`.
- Slint UI changes don't take effect without re-running `cargo build` — `build.rs` regenerates the bindings.
- Configuration migrations: add new fields with `#[serde(default)]` so older `config.json` files keep deserializing.
- Single-instance enforcement uses a named mutex (`src/utils/single_instance.rs`); don't add competing locks.
- The release profile uses `panic = "abort"` and `strip = "symbols"`; don't rely on `catch_unwind` or symbolicated backtraces in release builds.

## References

- `README.md` — user-facing install and behavior summary; useful for matching naming/wording in UI strings.
- `.github/workflows/ci.yml` — authoritative source of CI commands and the exact integration-test ordering.
- `.github/workflows/miri.yml` — read before changing `src/error.rs` or other portable modules; Miri runs on the `error` module and is sensitive to UB.
- `.github/workflows/security.yml` and `deny.toml` — read before adding a dependency or changing licenses; `cargo deny check` denies unknown registries and git sources.
- `.augment/rules/rust-dev-guidelines.md` — Rust 2024 / 1.93 idioms followed in this repo (lifetime capture, `let` chains, `Cargo.toml` lints config). Read before large refactors.
- `benches/` files — read before changing hot paths in `config`, `hdr`, `monitor`, `utils/icon_cache`, or `uwp`; baselines exist and >5% regressions are noteworthy.
