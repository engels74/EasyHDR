# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Codebase Search

**Always use the `mcp__auggie-mcp__codebase-retrieval` tool as the primary method for:**
- Exploring the codebase and understanding architecture
- Finding existing patterns before implementing new features
- Locating relevant code when the exact file location is unknown
- Gathering context before making edits
- Planning tasks in plan mode

This semantic search tool provides better results than grep/find for understanding code relationships. Use grep only for finding exact string matches or all occurrences of a known identifier.

## Project Overview

EasyHDR is a Windows-only application that automatically toggles HDR (High Dynamic Range) on/off based on running applications. It uses Windows Display Configuration APIs with version-specific implementations for Windows 10, Windows 11, and Windows 11 24H2+.

**Key Requirements:**
- Windows 10 21H2+ (build 19044 minimum)
- HDR-capable display
- Windows-specific APIs (application will not run on macOS/Linux)

## Build Commands

```bash
# Standard development build (with some optimization)
cargo build

# Release build (optimized, LTO enabled, stripped)
cargo build --release

# Format check
cargo fmt --all -- --check

# Linting
cargo clippy --all-targets --all-features -- -D warnings

# Run unit tests (parallel)
cargo test --lib --release

# Run integration tests (MUST be sequential - Windows API has global state)
cargo test --test integration_tests --release -- --test-threads=1
cargo test --test version_detection_tests --release -- --test-threads=1
cargo test --test memory_usage_test --release -- --test-threads=1

# Run all benchmarks
cargo bench

# Run specific benchmark
cargo bench --bench config
cargo bench --bench hdr_detection
cargo bench --bench icon_cache
cargo bench --bench process_monitor_bench
cargo bench --bench uwp_detection
```

## Testing

**CRITICAL:** Integration tests MUST run with `--test-threads=1` because they manipulate Windows API global state (HDR settings). Running in parallel will cause race conditions and test failures.

Unit tests can run in parallel, but integration tests in the `tests/` directory require sequential execution.

## Fuzzing

Requires Rust nightly toolchain:

```bash
# Install cargo-fuzz
cargo install cargo-fuzz

# Run fuzz targets (60 seconds each)
cargo +nightly fuzz run fuzz_config_json -- -max_total_time=60
cargo +nightly fuzz run fuzz_process_name -- -max_total_time=60
cargo +nightly fuzz run fuzz_windows_api -- -max_total_time=60
```

## Profiling

Special profiling build with debug symbols and frame pointers:

```bash
# Build with profiling profile
RUSTFLAGS="-C force-frame-pointers=yes" cargo build --profile profiling

# For tests
RUSTFLAGS="-C force-frame-pointers=yes" cargo build --profile profiling --tests
```

## Architecture

### Multi-threaded Event-Driven Design

The application uses a multi-threaded architecture with message passing between components:

1. **Main Thread**: Runs the Slint GUI event loop ([gui/gui_controller.rs](src/gui/gui_controller.rs))

2. **ProcessMonitor Thread**: Polls running processes every 500-1000ms to detect configured applications ([monitor/process_monitor.rs](src/monitor/process_monitor.rs))
   - Supports both Win32 applications and UWP/AppX packages
   - Sends `ProcessEvent` messages when apps start/stop

3. **AppController Thread**: Coordinates HDR state changes with debouncing ([controller/app_controller.rs](src/controller/app_controller.rs))
   - Receives `ProcessEvent` messages from ProcessMonitor
   - Receives `HdrStateEvent` messages from HdrStateMonitor
   - Manages HDR toggling with 500ms debounce to prevent rapid state changes
   - Sends `AppState` updates to GUI

4. **HdrStateMonitor Thread**: Monitors HDR state changes from external sources ([monitor/hdr_state_monitor.rs](src/monitor/hdr_state_monitor.rs))
   - Detects when users manually toggle HDR via Windows settings
   - Sends events to AppController to keep UI in sync

### Key Components

**HDR Control** ([hdr/](src/hdr/)):
- `controller.rs`: Core HDR enable/disable logic using Windows Display Configuration API
- `windows_api.rs`: Low-level Windows API bindings for HDR control
- `version.rs`: Windows version detection (Win10/Win11/Win11 24H2+) with different API implementations

**Configuration** ([config/](src/config/)):
- `models.rs`: Data structures for app config, monitored apps, user preferences
- `manager.rs`: Config loading/saving to `%APPDATA%\EasyHDR\config.json`
- Uses `serde_json` for serialization

**Process Detection** ([monitor/](src/monitor/)):
- `process_monitor.rs`: Win32 process enumeration via ToolHelp32 API
- `AppIdentifier`: Supports both Win32 exe paths and UWP package family names
- See [uwp/](src/uwp/) for UWP-specific process detection

**Error Handling** ([error.rs](src/error.rs)):
- Uses `thiserror` for structured errors with proper error chains
- All errors preserve source via `#[source]` for observability
- User-friendly error messages for common failures

**Utilities** ([utils/](src/utils/)):
- `icon_cache.rs`: Caches application icons as PNG (32x32 RGBA) using atomic writes
- `icon_extractor.rs`: Extracts icons from Win32 executables and UWP packages
- `autostart.rs`: Windows Registry integration for startup entry
- `single_instance.rs`: Named mutex to prevent multiple instances
- `logging.rs`: Tracing-based logging with file rotation
- `memory_profiler.rs`: Memory usage tracking (Windows only)
- `startup_profiler.rs`: Startup phase timing measurement
- `update_checker.rs`: GitHub releases API integration for version checks

### UI Layer

The UI is built with [Slint](https://slint.dev/) (declarative UI framework):
- UI definition: [ui/main.slint](ui/main.slint)
- Controller binding: [src/gui/gui_controller.rs](src/gui/gui_controller.rs)
- System tray integration: [src/gui/tray.rs](src/gui/tray.rs)

Uses Skia renderer (`renderer-skia` feature) for better font rendering on Windows vs the default FemtoVG renderer.

### Build System

[build.rs](build.rs) performs:
1. Compiles Slint UI files to Rust code via `slint-build`
2. Embeds version metadata and Git commit SHA
3. On Windows: embeds icon, version info, and manifest via `winres`

## Code Style

- Lints: `unsafe_code = "warn"`, `missing_docs = "warn"`, Clippy `pedantic` enabled
- Document all public APIs with `///` doc comments
- Use `tracing` for logging (not `println!` or `eprintln!`)
- Preserve error chains with `#[source]` in error types
- Use `parking_lot` for locks instead of `std::sync` (better performance)
- Use `Arc<RwLock<T>>` for shared state, `Arc<Mutex<T>>` for interior mutability

## Windows-Specific Considerations

- The `#[cfg(windows)]` attribute gates all Windows API code
- FFI calls use `#[expect(unsafe_code)]` with safety documentation
- Integration tests require Windows and will fail on other platforms
- The application sets `#![windows_subsystem = "windows"]` in release mode to hide console

## UWP Application Support

UWP/AppX applications (like Windows Store apps) require special handling:
- Package enumeration via `PackageManager` (WinRT API)
- Icon extraction via `GetLogo()` and stream decoding
- Package family names instead of exe paths
- See [src/uwp/](src/uwp/) for implementation details

## Configuration Storage

User configuration is stored at `%APPDATA%\EasyHDR\config.json` with:
- List of monitored applications (Win32 paths or UWP package family names)
- User preferences (monitoring interval, auto-start, etc.)
- Icon cache in `%APPDATA%\EasyHDR\icon_cache\` (PNG files named by UUID)

## CI/CD

GitHub Actions workflows in [.github/workflows/](.github/workflows/):
- `ci.yml`: Build, test, Clippy, format checks, VirusTotal scanning
- `profiling.yml`: Performance profiling with benchmarks and DHAT
- `security.yml`: Dependency auditing with `cargo-audit`
- `miri.yml`: Undefined behavior detection with Miri
- `release.yml`: Automated releases on version tags

All workflows run on `windows-latest` runners.

## Performance Targets

Baseline benchmarks (measured with Criterion):
- Config serialization: ~20 µs
- Config deserialization: ~35 µs
- Config round trip: ~55 µs
- HDR state detection: Platform-dependent

Monitor for >5% regressions when making changes.
