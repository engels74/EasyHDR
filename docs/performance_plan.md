# Performance Optimization Plan

**Status:** Requires Phase 0 Baseline Measurements
**Target:** 40-60% CPU reduction, 95% allocation reduction, 70-85% latency improvement
**Guidelines:** See [.claude/rust-dev-guidelines.md](../.claude/rust-dev-guidelines.md) for Rust best practices

**Critical Updates (Post-Review):**
- Phase 1.1: Changed `Mutex` → `RwLock` for concurrent reads in hot loop
- Phase 1.1 + 3.2: Added cache synchronization protocol between `ProcessMonitor` and `AppController`
- Phase 1.2: Added PID reuse property test + cache hit/miss instrumentation
- Phase 0 + 4.1: Added flamegraph interpretation criteria and workload diversity benchmarks
- Phase 4.4: Added specific test app list and cross-version testing strategy (Win10/11/11-24H2+)

---

## Executive Summary

Current bottleneck: Process monitor makes **300-500 Windows API calls** and **200-500 string allocations per second**. This plan reduces both by **90%+** through filtering, caching, and lock-free patterns.

### Key Metrics

| Metric | Baseline (Phase 0) | Target | Improvement |
|--------|-------------------|--------|-------------|
| API calls/poll | TBD | 5-20 | 90% ↓ |
| Allocations/sec | TBD | 5-10 | 95% ↓ |
| Poll latency | TBD | 5-15ms | 85% ↓ |
| Memory/poll | TBD | <100B | 95% ↓ |

---

## Phase 0: Baseline Profiling (Week 0)

**Goal:** Measure actual performance before optimization
**Status:** ✅ **BASELINE ESTABLISHED** - Ready for Phase 1 optimization

### Baseline Results Summary

**Flamegraph:** [docs/phase0/cpu-profiling-flamegraph/cpu-flamegraph.svg](../docs/phase0/cpu-profiling-flamegraph/cpu-flamegraph.svg) | **Total Samples:** 2,480 (30-second run)

| Function | % CPU | Analysis |
|----------|-------|----------|
| **`poll_processes`** | **90.9%** | ✅ **THE bottleneck** - Phase 1.1 will target this |
| `detect_uwp_process` | 9.96% | 🎯 **90% reduction expected** via early filtering |
| `OpenProcess` (Win32 API) | 5.44% | Required for UWP (cannot optimize) |
| `AppController::run` | 1.94% | ✅ Event handling is NOT a bottleneck |

**Key Finding:** `poll_processes` consumes **90.9% CPU** because it calls `detect_uwp_process` for ALL ~230 enumerated processes. **Phase 1.1 filtering will eliminate 90%+ of these calls** by checking monitored apps first → **~9% CPU savings**.

**Optimization Priority:** Phase 1.1 (Post-identification filtering) confirmed as highest-value optimization.

---

### Quick Start: Run Profiling

**Automated (GitHub Actions):**
```bash
# Push to performance branch to trigger profiling
git checkout -b feat/perf-phase-0-baseline
git push origin feat/perf-phase-0-baseline

# Artifacts: cpu-profiling-flamegraph-{sha}.zip, criterion-benchmarks-{sha}.zip
```

**Local (Windows, optional):**
```powershell
# Generate flamegraph locally
$env:RUSTFLAGS = "-C force-frame-pointers=yes"
cargo flamegraph --profile profiling --test cpu_profiling_test --output cpu-flamegraph.svg -- --exact --nocapture profile_process_monitoring_hot_paths

# View: Open cpu-flamegraph.svg in browser, search for "poll_processes"
```

See [profiling_guide.md](profiling_guide.md) for DHAT allocation profiling, Criterion benchmarks, and troubleshooting.

---

### AI-Assisted Flamegraph Analysis (Quick Reference)

**Claude can analyze SVG flamegraphs directly** - upload the SVG file (NOT PNG) for best results.

**What Claude extracts:**
- Hotspots (box width = CPU %)
- Call stacks and execution paths
- Allocation patterns (`String::from`, `Vec::push`)
- Lock contention (`Mutex::lock`, `RwLock`)

**Example prompts:**
```
"Identify the top 5 CPU hotspots in poll_processes"
"What % of CPU is CreateToolhelp32Snapshot vs String allocations?"
"Find all lock contention (Mutex, RwLock)"
```

**Critical requirement:** Flamegraph must show function names (e.g., `easyhdr::monitor::process_monitor::poll_processes`), NOT raw addresses (`0x7FFACB...`).

**Current CI flamegraph status:** ✅ EasyHDR functions symbolicated, ⚠️ Windows APIs show raw addresses (acceptable - we can infer from context).

**AI Analysis Capabilities:**
- ✅ Identify EasyHDR hotspots (e.g., `poll_processes` at 90.9%)
- ✅ Measure CPU time percentages from box widths
- ✅ Trace call stacks (e.g., `poll_processes` → `detect_uwp_process` → `OpenProcess`)
- ⚠️ Windows API names may need manual lookup if unsymbolicated

**Why SVG > PNG:**
- Text preserved (no OCR errors)
- Precise measurements (exact percentages)
- Hierarchical structure (call stack relationships)

**If file too large (>300KB):**
- Use browser search to find top hotspots first
- Generate shorter profile (10-15s instead of 30s)
- Ask specific questions instead of "analyze everything"

---

### Success Criteria

- [x] **`poll_processes` identified as hotspot (>20% CPU)** - **ACHIEVED: 90.9%**
- [x] **EasyHDR functions symbolicated** - **ACHIEVED** (Windows APIs partially unsymbolicated, acceptable)
- [x] **Optimization targets confirmed** - **Phase 1.1 filtering is highest-value** (~9% CPU gain)
- [ ] DHAT allocation profiling completed (200-500 allocs/sec baseline)
- [ ] Criterion benchmarks with varying workloads (1/5/10/50 apps)
- [ ] Hot paths documented ✅ (see Baseline Results above)

---

## Phase 1: API Call Reduction (Week 1)

**Goal:** Reduce Windows API calls by 90%

### 1.1 Post-Identification Filtering
**File:** `src/monitor/process_monitor.rs:190-268`
**Guideline:** *Prefer iterator adapters; avoid Mutex in hot loops* ([rust-dev-guidelines.md:96,119](../.claude/rust-dev-guidelines.md))

**CRITICAL:** Cannot filter before `OpenProcess` - UWP apps require process handle for detection via `detect_uwp_process(handle)`.

**Actions:**
1. Add **shared** `monitored_identifiers: Arc<RwLock<HashSet<AppIdentifier>>>` field to `ProcessMonitor`
2. Share same `Arc<RwLock<HashSet<AppIdentifier>>>` with `AppController` (see Phase 3.2 sync protocol)
3. Keep existing `OpenProcess` call (required for UWP detection at line 204)
4. After building `AppIdentifier` (Win32 or UWP), check `monitored_identifiers.read().contains(&app_id)`
5. Early-exit from loop iteration if not monitored (skip `insert` into `current_processes`)
6. Update cache in `update_watch_list()` via atomic swap: `*self.monitored_identifiers.write() = Arc::new(rebuild_set())`

**Cache Synchronization Protocol:**
- `ProcessMonitor` and `AppController` share the **same** `Arc<RwLock<HashSet<AppIdentifier>>>`
- When GUI modifies apps: `AppController` rebuilds and swaps via `.write()` lock
- `ProcessMonitor` reads via `.read()` lock (concurrent with event handling)

**Success Criteria:**
- [ ] Early exit reduces processing for ~230 unmonitored processes per poll
- [ ] RwLock allows concurrent reads in `poll_processes()` and `handle_process_event()`
- [ ] Both Win32 and UWP apps still detected correctly
- [ ] All integration tests pass with `--test-threads=1`

**Benchmark:**
```bash
cargo bench --bench process_monitor_bench
```

### 1.2 AppIdentifier Cache (Likely Needed - confirm in Phase 0)
**File:** `src/monitor/process_monitor.rs:442`
**Guideline:** *Pre-allocate (Vec::with_capacity); use SmallVec/ArrayVec for small typical sizes* ([rust-dev-guidelines.md:113](../.claude/rust-dev-guidelines.md))

**Actions:**
1. Add `app_id_cache: HashMap<u32, (AppIdentifier, Instant)>` field (PID → identifier + timestamp)
2. Use cache for repeated polls, expire entries >5s old (handles PID reuse)
3. Invalidate entries for PIDs not in current snapshot BEFORE diffing
4. Pre-allocate HashMap with capacity hint (200 processes)
5. **Instrumentation:** Add tracing for cache hit rate: `debug!("AppID cache: {}/{} hits ({:.1}%)", hits, total, hit_rate)`

**PID Reuse Safety:**
- Windows PIDs can be reused rapidly (especially on high-churn systems)
- 5s expiry may allow stale entries if PID reused within window
- **Mitigation:** Add property test `test_pid_reuse_rapid_churn()` simulating PID reuse within 2s

**Property-Based Testing:**
**Guideline:** *Use Proptest for invariant-heavy logic* ([rust-dev-guidelines.md:146](../.claude/rust-dev-guidelines.md))

Add property tests for concurrent cache operations:
```rust
proptest! {
    #[test]
    fn cache_coherence_concurrent_updates(updates in vec((0u32..10000, any::<String>()), 0..100)) {
        // Verify cache reads reflect most recent writes under concurrent access
    }

    #[test]
    fn debounce_window_invariant(event_timings_ms in vec(0u64..1000, 0..50)) {
        // Ensure 500ms debounce window respected across event sequences
    }
}
```

**Success Criteria:**
- [ ] String allocations reduced from ~250/poll to <10/poll
- [ ] DHAT shows 95% reduction in allocation rate
- [ ] Cache hit rate >80% after steady state (logged via tracing)
- [ ] **Property test validates PID reuse within 5s window** (stale detection)
- [ ] Property tests validate cache coherence and debounce timing (256 cases)

**Validation:**
```bash
cargo test --lib --release app_id_cache
cargo test --lib --release test_pid_reuse_rapid_churn
DHAT_PROFILER=1 cargo test --release
```

**Note:** Only implement if Phase 0 flamegraph shows string allocation overhead is significant (highly likely based on ~250 allocations/poll).

---

## Phase 2: Lock Contention Elimination (Week 2)

**Goal:** Remove allocation overhead and lock contention

### 2.1 Double-Arc Watch List
**File:** `src/monitor/process_monitor.rs:68,296-299`
**Guideline:** *Profile first: many clones are memcpy/RC bumps and not bottlenecks* ([rust-dev-guidelines.md:116](../.claude/rust-dev-guidelines.md))

**Actions:**
1. Change `watch_list: Arc<Mutex<Vec<MonitoredApp>>>` to `Arc<Mutex<Arc<Vec<MonitoredApp>>>>`
2. Update `update_watch_list()`: `*guard = Arc::new(apps)` instead of `*guard = apps`
3. In `poll_processes()`: `Arc::clone(&*guard)` instead of `guard.clone()`
4. Update `ProcessMonitor::new()` and all test helpers

**Error Handling:**
**Guideline:** *Libraries expose structured errors (thiserror); applications use anyhow with context* ([rust-dev-guidelines.md:47-49](../.claude/rust-dev-guidelines.md))
- Lock poisoning: Use `.expect("descriptive message")` for unrecoverable errors
- Rationale: Poisoning indicates panic in another thread; EasyHDR treats as unrecoverable
- Example: `.expect("watch_list lock poisoned - unrecoverable thread panic")`

**Success Criteria:**
- [ ] Per-poll allocation reduced from 2KB to 0 bytes
- [ ] Lock hold time: O(n) → O(1) (measure with `tracing`)
- [ ] Benchmark shows <5% overhead for Arc operations

### 2.2 Atomic Timestamp for Debouncing
**File:** `src/controller/app_controller.rs:44,339,426`
**Guideline:** *Document memory ordering explicitly for unsafe/atomics* ([rust-dev-guidelines.md:96-100](../.claude/rust-dev-guidelines.md))

**Actions:**
1. Replace `last_toggle_time: Arc<Mutex<Instant>>` with `last_toggle_time_nanos: Arc<AtomicU64>`
2. Add `startup_time: Instant` field for relative calculations
3. Use `Ordering::Relaxed` with comprehensive safety comment:
   ```rust
   // SAFETY: Relaxed ordering is sufficient for debouncing:
   // - Atomics guarantee cross-thread visibility even with Relaxed ordering
   // - No happens-before synchronization needed (approximate timing acceptable)
   // - Read/write don't synchronize other data structures
   // - Worst case: debounce window slightly off (acceptable for 500ms threshold)
   // - u64 nanos wraps after ~584 years (non-issue for debouncing)
   ```
4. Update reads: `startup_time + Duration::from_nanos(load(Relaxed))`
5. Update writes: `store(elapsed().as_nanos() as u64, Relaxed)`

**Success Criteria:**
- [ ] Zero mutex contention for timestamp operations
- [ ] Debounce logic still prevents rapid toggling (<500ms)
- [ ] Memory ordering safety comment added
- [ ] All debounce tests pass

**Test:**
```bash
cargo test --lib debounce
cargo test --test integration_tests --release -- --test-threads=1
```

---

## Phase 3: Read-Heavy Optimizations (Week 3)

**Goal:** O(n) → O(1) lookups, concurrent reads

### 3.1 RwLock for Config Access
**File:** `src/controller/app_controller.rs:30,249,298`
**Guideline:** *std::sync::Mutex only when no .await occurs* ([rust-dev-guidelines.md:96](../.claude/rust-dev-guidelines.md))

**Actions:**
1. `use parking_lot::RwLock;` (faster than std)
2. Change `config: Arc<Mutex<AppConfig>>` to `Arc<RwLock<AppConfig>>`
3. Update reads: `config.read()` (concurrent)
4. Update writes: `config.write()` (exclusive)
5. Update all test helpers
6. Lock poisoning error handling: `.expect("config lock poisoned - unrecoverable")`

**Success Criteria:**
- [ ] Multiple ProcessEvent handlers can read config concurrently
- [ ] Benchmark shows 60-80% reduction in config lock contention
- [ ] All tests pass without deadlocks

### 3.2 Pre-computed Monitored App HashSet
**File:** `src/controller/app_controller.rs:249-263,298-312`
**Guideline:** *Prefer iterator adapters over manual indexing* ([rust-dev-guidelines.md:119](../.claude/rust-dev-guidelines.md))

**Actions:**
1. Use **shared** `monitored_identifiers: Arc<RwLock<HashSet<AppIdentifier>>>` from Phase 1.1
2. Add `rebuild_monitored_identifiers()` method
3. Call rebuild on: `add_application()`, `remove_application()`, `toggle_app_enabled()`
4. Rebuild updates **both** `AppController` and `ProcessMonitor` via shared `Arc<RwLock<_>>`
5. Replace O(n) `.any()` with O(1) `monitored_identifiers.read().contains(&app_id)`

**Cache Synchronization (from Phase 1.1):**
- Same `Arc<RwLock<HashSet<AppIdentifier>>>` used in both `ProcessMonitor::poll_processes()` and `AppController::handle_process_event()`
- Atomic swap on rebuild: `*self.monitored_identifiers.write() = Arc::new(rebuild_set())`

**Success Criteria:**
- [ ] Event handling latency reduced by 50-70%
- [ ] Benchmark: `handle_process_event` <100µs
- [ ] Cache stays synchronized between `ProcessMonitor` and `AppController`
- [ ] Cache invalidation tests pass

---

## Phase 4: Validation & Tuning (Week 4)

### 4.1 Comprehensive Benchmarks
**New file:** `benches/process_monitor_bench.rs`
**Guideline:** *Use cargo-flamegraph for CPU hotspot identification (Phase 0-3); consider samply for timeline analysis in Phase 4+ if needed; DHAT for allocations* ([rust-dev-guidelines.md:109-110](../.claude/rust-dev-guidelines.md))

**Actions:**
1. **Flamegraph CPU profiling**: Verify `poll_processes` <20% CPU, `handle_process_event` <5% CPU
   ```bash
   cargo flamegraph --profile profiling --test cpu_profiling_test --output flamegraph.svg -- --exact --nocapture profile_process_monitoring_hot_paths
   ```
2. Benchmark `poll_processes()` full cycle with **varying workloads**:
   - Process counts: 100, 250, 500 processes
   - Monitored apps: 1, 5, 10, 50 apps
3. Benchmark `watch_list` clone (old) vs Arc clone (new)
4. Benchmark `handle_process_event()` latency
5. Benchmark config access (Mutex vs RwLock)
6. Compare before/after baselines
7. **Use `std::hint::black_box()`** to prevent optimizer pre-computation ([rust-dev-guidelines.md:146](../.claude/rust-dev-guidelines.md))
   ```rust
   use std::hint::black_box;
   c.bench_function("poll", |b| b.iter(|| black_box(monitor.poll_processes())))
   ```

**Command:**
```bash
cargo bench --baseline before
# (apply optimizations)
cargo bench --baseline after
cargo bench --load-baseline before --baseline after
```

### 4.2 Memory Profiling
**Guideline:** *Use DHAT for allocations* ([rust-dev-guidelines.md:109](../.claude/rust-dev-guidelines.md))

**Actions:**
1. Run DHAT on current implementation
2. Run DHAT after each phase
3. Verify 95% allocation reduction
4. **Memory leak detection** during 24-hour test (Phase 4.4):
   - Monitor RSS via Task Manager / Process Explorer
   - Plot memory usage over 24h (expect stable after startup)
   - Run Dr. Memory or WSL2 Valgrind for leak analysis

**Commands:**
```bash
RUSTFLAGS="-C force-frame-pointers=yes" cargo build --profile profiling
# DHAT allocation profiling
valgrind --tool=dhat ./target/profiling/easyhdr
# Memory leak detection (24h test)
drmemory -light -- ./target/release/easyhdr.exe
```

### 4.3 Channel Capacity Tuning (Low Priority)
**Files:** `src/controller/app_controller.rs`, `src/monitor/process_monitor.rs`
**Guideline:** *Only optimize if flamegraph shows cache locality issues* ([rust-dev-guidelines.md:109](../.claude/rust-dev-guidelines.md))

**Actions:**
1. Reduce channel capacity from 32 to 8-16
2. **Monitor for backpressure**: Add logging for `send_timeout()` failures
3. Measure cache locality improvements (expect minimal gain)

**Success Criteria:**
- [ ] No channel send failures under normal load (log warnings if blocked)
- [ ] No deadlocks from backpressure
- [ ] 6-9KB memory savings (minor improvement)

### 4.4 Real-World Validation (CRITICAL)
**Goal:** Verify optimizations work with actual applications and long-running scenarios

**Test Applications List:**
- **Games:** Steam, Epic Games Launcher, Cyberpunk 2077 / Forza Horizon 5
- **Creative Tools:** Adobe Premiere Pro / DaVinci Resolve, Blender
- **Streaming:** OBS Studio
- **UWP Apps:** Microsoft Store games (e.g., Minecraft Windows 10 Edition)

**Actions:**
1. **Real application testing**: Test HDR toggling with 3-5 apps from list above
2. **Long-running stability**: 24-hour stress test with cache invalidation, HDR toggling
   - Monitor RSS via Task Manager (expect stable after startup)
   - Run Dr. Memory leak detection (see Phase 4.2)
3. **Cross-version testing**: Verify on all supported Windows versions
   - **Windows 10 21H2+** (Build 19044): `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO`
   - **Windows 11 21H2-23H2** (Build 22000-22631): Same API as Win10
   - **Windows 11 24H2+** (Build 26100+): `DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO_2`
   - **Strategy:** Use GitHub Actions matrix builds or Azure VMs for cross-version testing
4. **Cache correctness**: Verify PID reuse, expired entries, and invalidation timing
   - Manual test: Rapidly start/stop monitored app (trigger PID reuse scenarios)
   - Verify cache hit rate >80% via tracing logs

**Success Criteria:**
- [ ] HDR toggles correctly with 3-5 real applications from test list
- [ ] No cache corruption after 24 hours (PID reuse handled correctly)
- [ ] No memory leaks (stable RSS plotted over 24h)
- [ ] Atomic timestamp doesn't drift
- [ ] All 3 Windows version variants work correctly (10, 11, 11 24H2+)

---

## Implementation Checklist

### Phase 0 (Baseline - REQUIRED FIRST)
- [ ] Run flamegraph CPU profiling
- [ ] Run DHAT allocation profiling
- [ ] Create Criterion baseline benchmarks
- [ ] Document hot paths and baseline metrics
- [ ] Create feature branch: `feat/performance-optimization`

### Phase 1 (API Reduction)
- [ ] 1.1 Post-identification filtering with RwLock implemented
- [ ] 1.1 Shared cache synchronization protocol established (ProcessMonitor ↔ AppController)
- [ ] 1.2 AppIdentifier cache with PID reuse test implemented (if Phase 0 confirms)
- [ ] 1.2 Cache hit rate instrumentation added (tracing)
- [ ] UWP detection still works correctly
- [ ] Integration tests pass

### Phase 2 (Lock Elimination)
- [ ] 2.1 Double-Arc watch list implemented
- [ ] 2.2 Atomic timestamp implemented
- [ ] Memory profiling shows 95% allocation reduction
- [ ] All tests pass

### Phase 3 (Read Optimization)
- [ ] 3.1 RwLock for config implemented
- [ ] 3.2 Monitored app HashSet using shared cache from Phase 1.1
- [ ] 3.2 Cache synchronization verified (ProcessMonitor ↔ AppController)
- [ ] Benchmarks show 50-70% event handling improvement
- [ ] All tests pass

### Phase 4 (Validation)
- [ ] 4.1 Comprehensive benchmarks with varying workloads (100/250/500 processes, 1/5/10/50 apps)
- [ ] 4.1 Flamegraph confirms `poll_processes` <20% CPU, `handle_process_event` <5% CPU
- [ ] 4.2 Memory profiling completed (DHAT + leak detection)
- [ ] 4.2 24-hour stability test shows stable RSS (no leaks)
- [ ] 4.3 Channel capacity tuned with backpressure monitoring (optional)
- [ ] 4.4 Real-world validation with 3-5 apps from test list (CRITICAL)
- [ ] 4.4 Cross-version testing on Win10/11/11-24H2+ via GitHub Actions/VMs
- [ ] Documentation updated

### Final Validation
- [ ] Phase 0 baseline documented
- [ ] All benchmarks show target improvements vs baseline
- [ ] Flamegraph confirms hotspots resolved
- [ ] No performance regressions in any scenario
- [ ] 24-hour stability test passed
- [ ] Integration tests pass: `cargo test --test integration_tests --release -- --test-threads=1`
- [ ] Unit tests pass: `cargo test --lib --release`
- [ ] Clippy clean: `cargo clippy --all-targets --all-features -- -D warnings`
- [ ] PR ready for review

---

## Rollback Plan

Each phase is independently revertible:

1. **Phase 1 fail:** Revert filtering logic, keep existing `poll_processes()` implementation
2. **Phase 2 fail:** Revert to `Arc<Mutex<T>>` patterns, remove atomic timestamp
3. **Phase 3 fail:** Revert to `Mutex`, remove HashSet cache
4. **Phase 4 fail:** Restore default channel capacity (32)

**Git Strategy:**
- Create branch per phase: `feat/perf-phase-1`, `feat/perf-phase-2`, etc.
- Merge to main only after phase validation
- Tag baseline: `v0.1.4-perf-baseline`

---

## Success Metrics (Final)

| Metric | Baseline (Phase 0) | Target | Measured |
|--------|-------------------|--------|----------|
| **CPU Usage** | | | |
| Process monitor | ___ % | 10-20% | ___ % |
| Event handling | ___ % | 30-50% | ___ % |
| Overall app | ___ % | 40-60% | ___ % |
| **Memory** | | | |
| Allocation rate | ___ /s | 5-10/s | ___ /s |
| Per-poll alloc | ___ B | <100B | ___ B |
| Peak memory | ___ KB | -15KB | ___ KB |
| **Latency** | | | |
| Poll cycle | ___ ms | 5-15ms | ___ ms |
| Event handling | ___ µs | -50% | ___ µs |
| API calls/poll | ___ calls | 5-20 | ___ calls |

**Note:** Baseline column filled during Phase 0. Targets may adjust based on actual measurements.

---

## References

- **Rust Guidelines:** [.claude/rust-dev-guidelines.md](../.claude/rust-dev-guidelines.md)
  - **Profiling:** Lines 109-110 (cargo-flamegraph, DHAT)
  - **Allocation:** Line 113 (pre-allocate with capacity, SmallVec)
  - **Concurrency:** Lines 96-100 (Mutex vs RwLock, atomic ordering)
  - **Optimization:** Line 116 (profile before removing clones)
  - **Iterators:** Line 119 (prefer adapters over manual indexing)
  - **Testing:** Line 146 (Proptest for invariants, Criterion with black_box)
  - **Error Handling:** Lines 47-52 (anyhow for apps, thiserror for libraries)

### Architecture Decision: Threading vs Async

**Decision:** Thread-based concurrency with crossbeam channels
**Rationale:** Windows APIs (CreateToolhelp32Snapshot, OpenProcess, DisplayConfig) are inherently blocking; polling-based monitoring (500-1000ms) has no benefit from async task switching; thread overhead negligible for 2-3 threads. Adding Tokio would introduce runtime complexity without performance gains.
**Guideline Alignment:** "Never block the async runtime" ([rust-dev-guidelines.md:85-86](../.claude/rust-dev-guidelines.md)) — we avoid this by not using async where blocking is inherent.

---

**Estimated Total Time:** 4-5 weeks (includes Phase 0 baseline + Phase 4.4 real-world validation)
**Risk Level:** Low-Medium (Phase 0 reduces uncertainty; cache coordination requires care)
**Breaking Changes:** None (internal optimizations only)
