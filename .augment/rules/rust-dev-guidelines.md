---
type: "always_apply"
---

## Purpose

You are a Rust expert specializing in 2024–2025 best practices.

Deliver terse, enforceable rules for production Rust: sound ownership/lifetimes, elegant traits/generics, robust error handling, clean APIs, reliable Tokio concurrency (structured, cancel-safe, backpressure), data-driven performance, and strong supply-chain/unsafe hygiene—targeting Rust 1.85+ (Edition 2024).

## Code Design: Ownership, Lifetimes, Traits, Error Handling, API Design

### Modern Rust Language Features
- Use native async fn in traits (1.75+); avoid async-trait unless you need dyn or older compilers.  
  Rationale: Native async avoids heap boxing and clarifies types while improving performance; async-trait remains for dynamic dispatch and pre-1.75 support. Return Type Notation for some Send bounds is still unstable; design APIs accordingly.
  Snippet: `trait Service { async fn request(&self, k: i32) -> R; }`

- Use async closures (1.85+) for higher-ranked async bounds and borrowing from captures.  
  Rationale: `async || {}` enables lending from captures and signatures previously impossible with “returns future” closures. Prefer `async Fn*` bounds; use `|| async {}` only when a 'static future is desired.
  Snippet: `F: async FnMut(&str)`

### Ownership & Borrowing
- Default to &T for reads, &mut T for exclusive writes; use Cow when mutation is conditional.  
  Rationale: The borrow checker ensures single-writer OR multiple-readers for race freedom; Cow borrows when unchanged and owns only on modification. Use Rc/Arc only when shared ownership is truly necessary due to runtime overhead.
  Snippet: `fn f(input: Cow<str>) -> Cow<str>`

- Rely on lifetime elision; be explicit only to express real lifetime relationships.  
  Rationale: Elision covers most cases; explicit lifetimes should signal outputs tied to non-receiver inputs or multiple distinct lifetimes. Over-annotation obscures intent and fights inference.
  Snippet: `impl<'src> Scanner<'src> { fn next(&mut self) -> Token<'src> }`

### Traits & Generics
- Return impl Trait for static dispatch; use dyn Trait for runtime heterogeneity or code size.  
  Rationale: impl Trait monomorphizes to zero-cost specialized code; dyn adds small vtable overhead but supports heterogeneous collections and smaller binaries. Choose by compile-time knowledge vs runtime flexibility.

- Implement From<T> on your types; accept Into<T>/AsRef<T> in parameters.  
  Rationale: From auto-enables Into via blanket impls; Into/AsRef inputs improve ergonomics and ownership flexibility. Use TryFrom/TryInto for fallible conversions; prefer AsRef for cheap ref-to-ref views.
  Snippet: `fn process<D: Into<MyType>>(d: D)`

- Keep traits single-responsibility; compose bounds instead of “god traits.”  
  Rationale: Focused traits are easier to implement and reason about, and yield better diagnostics. Compose with `T: A + B` when concepts are separable.

- Use associated types when there’s one answer per implementor; use generics to allow multiple impls.  
  Rationale: Associated types improve ergonomics and enforce singular relationships (e.g., `Iterator::Item`); generics allow multiple distinct implementations (e.g., `From<&str>`, `From<String>`).

### Error Handling
- Libraries expose structured errors (thiserror); applications use anyhow with context.  
  Rationale: Libraries need matchable variants; apps need ergonomic propagation and user-facing messages/backtraces. This division maximizes utility and clarity across consumers and executables.
  Snippet: `err.context("while parsing config")?`

- Preserve error source chains; prefer .context() over string formatting.  
  Rationale: Source chains retain causal history and programmatic inspection; ad‑hoc strings duplicate or lose structure. Hide internal details only across security boundaries.

- Use #[expect(..)] instead of #[allow(..)] for temporary suppressions.  
  Rationale: expect documents intended lint triggers and warns when the debt disappears; allow silently suppresses. Reserve allow for permanent, intentional policy.

### API Design
- Use conversion naming consistently: as_ (cheap ref), to_ (clone/expensive), into_ (consumes).  
  Rationale: Standard vocabulary signals cost and ownership change, reducing accidental inefficiency and misuse.

- Eagerly derive Debug, Clone, PartialEq, Eq, Hash on public types; use newtypes for domain safety.  
  Rationale: Avoid future breaking changes and enable diagnostics/collections; newtypes prevent parameter mix-ups in APIs.

- Accept generic inputs (Into/AsRef), return concrete types (or impl Trait when hiding unnameable types).  
  Rationale: Flexible inputs maximize ergonomics; concrete outputs avoid boxing/leaks and aid chaining. Use impl Trait to hide types without sacrificing static dispatch.

- Enforce configuration correctness via typestate builders for required fields.  
  Rationale: Builders that encode state in types eliminate runtime “missing required field” errors and move failures to compile time.

## Concurrency & Async: Tokio Patterns, Structured Concurrency, Cancellation, Backpressure

### Tokio Runtime Configuration
- Configure the runtime explicitly for production; don’t rely on defaults.  
  Rationale: Explicit worker/thread tuning, naming, and platform tweaks avoid starvation and clarify intent; multiple runtimes/flavors can be selected as needed. Consider disabling LIFO slot optimization to reduce starvation in message-passing workloads.
  Snippet: `Builder::new_multi_thread().worker_threads(n).enable_all().build()?`

### Structured Concurrency & Cancellation
- Use JoinSet for task lifecycles; aborts pending tasks on drop.  
  Rationale: Prevents detached tasks and offers cancel-safe joins inside select loops; use TaskTracker when you must wait for graceful completion instead of aborting.

- Use CancellationToken; model hierarchies with child tokens.  
  Rationale: Tokens propagate cooperative cancellation across task graphs without shared mutable state; children cancel with parents while remaining independently controllable. Use oneshot for single-signal cases; timeouts for pure request/response.

### Async Correctness
- Never block the async runtime; offload sync/CPU work via spawn_blocking.  
  Rationale: Blocking starves the executor and spikes latency; frequent .await keeps the system responsive. Tiny compute can inline—verify with profiling.

- Avoid select! on stateful futures; use only with idempotent operations or cancel-safe futures.  
  Rationale: Dropped branches lose in-flight state (parsers, file/stream positions), causing subtle bugs. Safe for timeouts, independent races, or documented cancel-safe ops (e.g., `JoinSet::join_next()`).

- Pin and reuse futures in select! loops to avoid recreation overhead.  
  Rationale: Recreating futures each iteration wastes allocations/initialization; pinning preserves state and reduces churn.
  Snippet: `tokio::pin!(ctrl_c);`

### Sync Primitives & Backpressure
- std::sync::Mutex only when no .await occurs; use tokio::sync::Mutex across awaits.  
  Rationale: std::sync blocks threads and guards are non-Send across awaits, risking deadlocks; tokio’s mutex yields to the executor when contended. Default to tokio’s when unsure.

- Use bounded mpsc channels to enforce backpressure.  
  Rationale: Capacity bounds throttle producers and cap memory, aligning with Tokio’s poll-driven laziness. Unbounded channels are acceptable only when producers are inherently limited—monitor memory.

## Performance: Profiling, Allocations, Zero-Cost Abstractions, Unsafe/FFI

### Profiling
- Enable frame pointers and debug info for profiling release builds.  
  Rationale: Accurate stacks drastically improve profiling insight with negligible runtime impact. Keep enabled in performance testing; strip for production distributions if needed.
  Snippet: `[profile.release] debug=true; RUSTFLAGS="-C force-frame-pointers=yes"`

- Use cargo-flamegraph (often with --root) for CPU; use DHAT for allocations.  
  Rationale: Flamegraphs visualize hot code paths (system call capture helps I/O); DHAT finds allocation/memcpy hotspots and peak memory usage. Use samply for cross-platform, heaptrack for quick glances.

### Allocation & Abstractions
- Pre-allocate (Vec::with_capacity); use SmallVec/ArrayVec for small typical sizes.  
  Rationale: Pre-sizing cuts realloc/copy churn; SmallVec keeps common small cases on stack; ArrayVec enforces a hard maximum with speed benefits. Choose by measured size distributions.

- Profile before removing clones; prefer borrowing/Cow only when hot.  
  Rationale: Many clones are memcpy/RC bumps and not bottlenecks; premature removal harms readability for negligible gains. Optimize only where profiling proves impact.

- Prefer iterator adapters over manual indexing; verify ultra-hot paths.  
  Rationale: Iterators compile to loops with zero-cost fusion and enable optimizations; use explicit loops only where clarity demands and performance is proven.

- Encode state with typestate/ZSTs when transitions are compile-time.  
  Rationale: ZST markers provide compile-time guarantees at zero runtime cost; reserve runtime checks for genuinely dynamic state.

### Unsafe & FFI
- Run unsafe code under Miri; test multiple targets (32-bit, big-endian); use many-seeds for nondeterminism.  
  Rationale: Miri catches UB beyond compiler checks; cross-arch testing reveals endian/width issues; varied seeds expose concurrency schedule bugs.

- Validate all FFI inputs and document Safety invariants explicitly.  
  Rationale: C/C++ lacks Rust’s safety; check nulls, lengths, and encodings; encapsulate unsafe behind safe abstractions with clear invariants.

## Tooling & QA: Cargo, Lints, Tests, Fuzzing, Benchmarks

### Cargo Workspaces & Features
- Use resolver="2"; keep features strictly additive; prefer dep: for optional deps.  
  Rationale: Prevents feature leakage/unintended activation across dev/build/platform boundaries; additive features compose safely; dep: clarifies feature namespaces and weak dependencies.

- Prefer flat workspace layout at scale; centralize versions in [workspace.dependencies].  
  Rationale: Improves navigation and consistency for 10K–1M LOC; members inherit versions via `workspace = true`, avoiding drift and duplication.

### Linting
- Configure workspace lints (1.74+); enable clippy::pedantic selectively.  
  Rationale: Centralized policy reduces duplication and aligns teams; keep performance/correctness pedantic lints, allow noisy ones with documented rationale.

### Testing & Benchmarks
- Use Proptest for invariant-heavy logic; cargo-fuzz for untrusted inputs; Criterion for rigorous perf.  
  Rationale: Property testing finds edge cases and shrinks failures; fuzzing uncovers deep parser/decoder crashes; Criterion adds statistical rigor and regression detection (`black_box` to avoid pre-optimization).

## Security: no_std, Memory Safety, Sandboxing, Supply-Chain

### Memory Safety
- Minimize unsafe; document # Safety invariants; encapsulate behind safe APIs.  
  Rationale: Memory-safety CVEs require unsafe; documenting invariants and scoping unsafe reduces risk; validate with Miri and sanitizers in CI.

### Supply-Chain
- Run cargo-audit in CI; use cargo-deny for licenses/duplicates; consider cargo-vet for critical projects; inspect with cargo-geiger.  
  Rationale: Catch known vulns early, enforce org policy, maintain audit trails where warranted, and assess unsafe usage and crate health before adoption.

### Embedded & no_std
- Make libraries no_std-compatible where feasible; gate std via features and test cross-compilation.  
  Rationale: Maximizes portability (embedded/WASM/kernel) and avoids accidental std dependencies; applications needing OS services can remain std.

### Sandboxing
- Use cap-std for capability-based filesystem access.  
  Rationale: Restricts file operations to granted directories, preventing traversal and symlink attacks; not a sandbox for untrusted Rust code, but strong for untrusted paths.

## Ecosystem: MSRV & Edition 2024

- Declare rust-version; target N–2 MSRV for libraries (broader where adoption requires).  
  Rationale: Clear compatibility expectations help users and CI; applications can use latest stable; communicate MSRV bumps explicitly.

- Migrate to Edition 2024 via cargo fix --edition; test per-crate in workspaces.  
  Rationale: Automated, low-risk migration with minimal breaking changes; key changes include RPIT lifetime capture, `gen` reserved, and prelude additions (Future/IntoFuture). Production-ready per Rust 1.85.0 release notes (2025‑02‑20).

## Critical 2025 Rules (at a glance)

1) thiserror for libraries; anyhow with .context() for apps.  
2) Tokio: JoinSet + CancellationToken; never block the runtime.  
3) std::sync::Mutex only without .await; use tokio::sync::Mutex across awaits.  
4) Miri for unsafe; validate FFI; test multiple targets and many-seeds.  
5) cargo-audit in CI; cargo-deny/cargo-vet where needed; check crates with cargo-geiger.  
6) Cargo: resolver="2", additive features, workspace lints, [workspace.dependencies].  
7) Profile first: frame pointers + flamegraph; DHAT for allocations; pre-allocate where sensible.  
8) Property tests, fuzz targets, Criterion benchmarks.  
9) Implement From; accept Into/AsRef; use consistent as_/to_/into_ naming.  
10) Edition 2024 migration with cargo fix --edition; declare rust-version (MSRV).
