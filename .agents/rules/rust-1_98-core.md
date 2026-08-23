---
type: "agent_requested"
description: "Rust 1.98 (edition 2024) coding guidelines"
---
# Rust 1.98 Core: Idiomatic Systems Code on the 2024 Edition

Rust 1.98.0 (stable, 20 August 2026 — the release whose headline is that `f32`/`f64` gained "algebraic" methods for reorderable add/sub/mul/div/rem) on the **2024 edition** is the baseline this document targets. Edition 2024 (default since Rust 1.85, 20 February 2025, which the Rust team called "the largest edition we have released") changed RPIT lifetime capture, made `unsafe_op_in_unsafe_fn` warn-by-default, turned references to `static mut` into a hard error, requires `unsafe extern` blocks, reserved `gen`, adjusted temporary/tail-expression drop order, and added `Future`/`IntoFuture` to the prelude. Optimize for: making illegal states unrepresentable in the type system, borrowing over cloning, returning `Result` with typed errors from libraries, and pushing correctness into `enum`s and newtypes so the compiler enforces your invariants.

The most common way agents write wrong-but-plausible Rust is by importing habits from garbage-collected or exception-based ecosystems: cloning to escape the borrow checker instead of restructuring ownership, reaching for `Arc<Mutex<_>>` as a default instead of passing `&mut`, using `unwrap()`/`panic!` for recoverable errors, hand-rolling error `enum`s instead of `thiserror`, adding the `async-trait` crate reflexively when native `async fn` in traits now works for static dispatch, and pulling in `once_cell`, `lazy_static`, or `chrono` when std `LazyLock`/`OnceLock` and `jiff` are the current answers. Write to the current stable surface; do not carry pre-2024 idioms.

## Project layout, Cargo manifest, and workspaces

A modern binary+library crate keeps logic in the library and a thin `main.rs`. Use a workspace with `[workspace.dependencies]` and a shared `[workspace.lints]` from day one — even for a single crate, it costs nothing and scales.

```toml
# Cargo.toml (workspace root)
[workspace]
members = ["crates/*"]
resolver = "3"                       # implied by edition 2024; state it explicitly for virtual workspaces

[workspace.package]
edition = "2024"
rust-version = "1.98"                # MSRV; the v3 resolver is MSRV-aware
license = "MIT OR Apache-2.0"

[workspace.dependencies]
# Single source of truth for versions across all member crates.
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"
thiserror = "2.0.20"
anyhow = "1.0.104"
tokio = { version = "1.53", features = ["rt-multi-thread", "macros"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter", "json"] }

[workspace.lints.rust]
unsafe_op_in_unsafe_fn = "warn"
missing_debug_implementations = "warn"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
```

```toml
# crates/app/Cargo.toml (member crate)
[package]
name = "app"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
serde.workspace = true
thiserror.workspace = true
tokio.workspace = true

[lints]
workspace = true                     # inherit the workspace lint config

[features]
default = []
# Additive features only: enabling a feature must never remove APIs or change behavior.
postgres = ["dep:sqlx"]
```

Critical insight: `edition = "2024"` implies `resolver = "3"` for the top-level package, which sets `resolver.incompatible-rust-versions = "fallback"` — Cargo prefers dependency versions compatible with your `rust-version`. Rust 1.84.0 (9 January 2025) stabilized this MSRV-aware resolver, which "prefers dependency versions compatible with the project's declared MSRV"; before edition 2024 you opted in via `.cargo/config.toml` with `[resolver]` / `incompatible-rust-versions = "fallback"`. The resolver setting is only honored in the workspace root; it is ignored in dependencies. `Cargo.lock` is v4 and should be committed for binaries and workspaces.

File-purpose map:

| Path | Purpose |
|------|---------|
| `src/lib.rs` | Crate root; public API surface, module declarations, `#![doc]` |
| `src/main.rs` | Thin binary entry: parse args, init tracing, call into the lib |
| `src/bin/*.rs` | Additional binaries |
| `tests/*.rs` | Integration tests; each file is its own crate, sees only public API |
| `benches/*.rs` | Criterion/Divan benchmarks (`[[bench]]`, `harness = false`) |
| `build.rs` | Build script; keep minimal, emit `cargo::rustc-*` instructions |
| `rust-toolchain.toml` | Pin toolchain channel + components for reproducible builds |
| `.cargo/config.toml` | Linker, target flags, aliases (local/CI, not published) |
| `deny.toml` | `cargo-deny` license/advisory/bans policy |

## The 2024 edition: what changes how you write code

**Precise capturing with `use<>` (stable 1.82).** In edition 2024, `-> impl Trait` implicitly captures *all* in-scope generic parameters including lifetimes. Use `use<>` to narrow it when the returned value does not actually borrow an input:

```rust
// Without use<>, the returned iterator would capture `prefix`'s lifetime,
// pinning the borrow. use<> says "capture nothing from the environment".
fn counter(n: usize) -> impl Iterator<Item = usize> + use<> {
    0..n
}

// Capture only what you need:
fn tagged<'a, T: Clone>(items: &'a [T], _cfg: &Config) -> impl Iterator<Item = T> + use<'a, T> {
    items.iter().cloned()
}
```

**`unsafe_op_in_unsafe_fn` warns by default.** An `unsafe fn` body no longer grants an implicit unsafe scope; wrap unsafe operations in explicit `unsafe {}` blocks with a safety comment.

**References to `static mut` are a hard error.** Use `AtomicUsize`, `Mutex`, `OnceLock`, or `LazyLock` instead.

**`unsafe extern` blocks required**, with per-item `safe`/`unsafe` qualifiers (see FFI section). **Unsafe attributes**: `#[no_mangle]`, `#[export_name]`, `#[link_section]` must be written `#[unsafe(no_mangle)]`.

**Tail-expression temporary scope changed.** Temporaries in a block's tail expression now drop *before* the block's locals, which fixes a class of borrow errors:

```rust
fn last(v: &RefCell<Vec<i32>>) -> Option<i32> {
    v.borrow().last().copied()   // the Ref temporary drops before returning; compiles cleanly in 2024
}
```

**`gen` is a reserved keyword** (native `gen` blocks are not yet stable; do not use them in production). **`Future` and `IntoFuture` are in the prelude.** The `expr` macro fragment specifier changed to also match `const` and `_` expressions (old behavior available as `expr_2021`).

Version-anchored language features worth using: **let-else (1.65)**, **let-chains (1.88, edition 2024)**, **`async fn`/RPIT in traits (1.75)**, **async closures & `AsyncFn` (1.85)**, **`#[expect(lint)]` (1.81)**, **`LazyLock`/`LazyCell` (1.80)**, **`OnceLock`/`OnceCell` (1.70)**, **inline `const {}` (1.79)**, **C-string literals `c"..."` (1.77)**, **`#[diagnostic::on_unimplemented]` (1.78)**, **naked functions (1.88)**.

```rust
// let-else: bind or diverge, no rightward drift.
let Ok(port) = env::var("PORT").unwrap_or_default().parse::<u16>() else {
    return Err(ConfigError::MissingPort);
};

// let-chains (1.88): mix `let` bindings and bool conditions with &&.
if let Some(user) = session.user() && user.is_admin && let Some(t) = user.token() {
    grant(t);
}
```

## Ownership, borrowing, and the `&str`/`String`/`Cow` decisions

Accept the most general borrowed form and return owned only when you must. Take `&str` not `&String`, `&[T]` not `&Vec<T>`, `impl AsRef<Path>` for filesystem paths. Return `String`/`Vec<T>` when you produce new data; borrow when you're a view.

```rust
fn greet(name: &str) -> String { format!("Hello, {name}") }        // borrow in, own out
fn first_word(s: &str) -> &str { s.split_whitespace().next().unwrap_or("") } // borrow through

// Cow: borrow in the common case, allocate only when you must mutate.
use std::borrow::Cow;
fn sanitize(input: &str) -> Cow<'_, str> {
    if input.contains('\0') {
        Cow::Owned(input.replace('\0', ""))   // rare path allocates
    } else {
        Cow::Borrowed(input)                  // common path is zero-copy
    }
}
```

`impl Trait` in argument position means "any type implementing this" (generic); in return position it means "one concrete hidden type". Use generics/`impl Trait` args for flexibility, `impl Trait` returns for iterators/closures/futures you don't want to name. Clone deliberately: if you clone to satisfy the borrow checker, first ask whether restructuring (split borrows, index-based access, `mem::take`, narrower scopes) removes the need.

## Type-driven design: make illegal states unrepresentable

Prefer enums over booleans and stringly-typed values; wrap primitives in newtypes; use the typestate pattern for protocols; seal traits you don't want implemented downstream.

```rust
// Newtype: a UserId is not an OrderId, even though both wrap u64.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UserId(u64);

// Enum over bool-soup: three booleans allow 8 states, only 3 are valid.
#[derive(Debug, Clone, Copy)]
pub enum Visibility { Public, Unlisted, Private }

// #[non_exhaustive] lets you add variants/fields later without a breaking change.
#[non_exhaustive]
#[derive(Debug)]
pub enum Event { Created, Updated, Deleted }

// Typestate: a connection that can only be queried once opened.
pub struct Conn<S> { sock: TcpStream, _state: std::marker::PhantomData<S> }
pub struct Closed;
pub struct Open;
impl Conn<Closed> {
    pub fn open(self) -> Conn<Open> { Conn { sock: self.sock, _state: std::marker::PhantomData } }
}
impl Conn<Open> {
    pub fn query(&self, _sql: &str) { /* only callable when Open */ }
}

// Sealed trait: implementable only within this crate.
mod sealed { pub trait Sealed {} }
pub trait Backend: sealed::Sealed { fn name(&self) -> &str; }
```

The builder pattern for structs with many optional fields (consider the `bon` crate's derive builder for large types, but a hand-written builder is fine):

```rust
#[derive(Debug, Default)]
pub struct ServerConfig { port: u16, workers: usize, tls: bool }
pub struct ServerConfigBuilder(ServerConfig);
impl ServerConfig {
    pub fn builder() -> ServerConfigBuilder { ServerConfigBuilder(Self::default()) }
}
impl ServerConfigBuilder {
    #[must_use] pub fn port(mut self, p: u16) -> Self { self.0.port = p; self }
    #[must_use] pub fn workers(mut self, w: usize) -> Self { self.0.workers = w; self }
    pub fn build(self) -> ServerConfig { self.0 }
}
```

## Traits, generics, and dyn vs generics

Generics monomorphize (zero-cost, larger binary, static dispatch); `dyn Trait` is one binary, virtual dispatch, and requires the trait be **dyn-compatible** (object-safe): no generic methods, no `Self`-returning methods, no async fn (see async section). Use generics by default; use `dyn` when you need heterogeneous collections or want to shrink compile time / binary size.

```rust
// Static dispatch: monomorphized, inlinable.
fn total<I: IntoIterator<Item = u64>>(xs: I) -> u64 { xs.into_iter().sum() }

// Dynamic dispatch: one type, heterogeneous storage.
let handlers: Vec<Box<dyn Fn(&Event)>> = vec![Box::new(|_| {}), Box::new(log_event)];
```

GATs (stable 1.65) let associated types carry a lifetime — the enabling feature for lending iterators:

```rust
trait LendingIterator {
    type Item<'a> where Self: 'a;
    fn next(&mut self) -> Option<Self::Item<'_>>;
}
```

## Error handling: `thiserror` for libraries, `anyhow` for binaries

Libraries define typed errors with **thiserror 2.x**; applications use **anyhow 1.x** for the erased, context-carrying error. Never `Box<dyn Error>` in a public library API — it erases match-ability. `failure`, `error-chain`, and `quick-error` are dead; do not use them.

```rust
// Library: a typed, matchable error enum. thiserror generates Display + Error + From.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("key not found: {0}")]
    NotFound(String),
    #[error("io error")]
    Io(#[from] std::io::Error),           // #[from] implies #[source] and generates From
    #[error("record {id} is corrupt")]
    Corrupt { id: u64 },
    #[error(transparent)]                  // forward Display+source unchanged
    Other(#[from] anyhow::Error),
}

pub fn load(path: &str) -> Result<Vec<u8>, StoreError> {
    let bytes = std::fs::read(path)?;      // std::io::Error -> StoreError via #[from]
    if bytes.is_empty() {
        return Err(StoreError::Corrupt { id: 0 });
    }
    Ok(bytes)
}
```

```rust
// Application: anyhow with .context() for a human-readable causal chain.
use anyhow::{Context, Result, bail, ensure};

fn run(cfg_path: &str) -> Result<()> {
    let raw = std::fs::read_to_string(cfg_path)
        .with_context(|| format!("reading config from {cfg_path}"))?;
    ensure!(!raw.is_empty(), "config file is empty");
    let cfg: Config = toml::from_str(&raw).context("parsing config as TOML")?;
    if cfg.port == 0 { bail!("port must be non-zero"); }
    Ok(())
}
```

Rules of thumb: return `Result` for anything a caller could reasonably handle; `panic!`/`unwrap`/`expect` only for programmer errors and truly-impossible states. Prefer `expect("clear invariant description")` over `unwrap()` — the message documents *why* it can't fail. `std::error::Error` and the `?`-friendly source chain are in `core::error` (usable in `no_std`).

## Iterators and closures

Chain adaptors; avoid intermediate `collect()`. Closures capture by the least-restrictive mode; the three traits are `FnOnce` (consumes captures), `FnMut` (mutates), `Fn` (shared). `move` forces capture-by-value (essential for spawned threads/tasks).

```rust
// Idiomatic: lazy, allocation-free until the terminal operation.
let sum_of_squares: u64 = (1..=100).filter(|n| n % 2 == 0).map(|n| n * n).sum();

// Fallible iteration collects into Result<Vec<_>, E>, short-circuiting on first Err.
let parsed: Result<Vec<i32>, _> = ["1", "2", "3"].iter().map(|s| s.parse::<i32>()).collect();

// itertools (0.14) for what std lacks: itertools::Itertools::chunk_by, unique, sorted, etc.
use itertools::Itertools;
let grouped = data.iter().chunk_by(|r| r.category);
```

Avoid `.iter().cloned().collect()` when you can borrow; avoid `.collect::<Vec<_>>().iter()` round-trips; use `iter_mut()` to mutate in place. For index math prefer `.enumerate()` over manual counters.

## Collections, hashing, and smart pointers

Default to `Vec<T>` and `HashMap<K, V>`. Std's `HashMap` uses a DoS-resistant SipHash; for internal, non-adversarial maps where speed matters, swap the hasher to **foldhash** (0.2) or `ahash`. `hashbrown` (0.16) is std's `HashMap` implementation and is only needed directly for its raw entry API or `no_std`.

```rust
use std::collections::HashMap;
use foldhash::fast::RandomState;
type FastMap<K, V> = HashMap<K, V, RandomState>;   // faster, non-cryptographic
```

Smart pointers and interior mutability, chosen precisely:

| Need | Single-thread | Multi-thread |
|------|---------------|--------------|
| Shared ownership | `Rc<T>` | `Arc<T>` |
| Interior mutability, one value | `RefCell<T>` | `Mutex<T>` |
| Interior mutability, read-heavy | `RefCell<T>` | `RwLock<T>` |
| Lazy one-time init | `OnceCell` / `LazyCell` | `OnceLock` / `LazyLock` |

Prefer **std `OnceLock`/`LazyLock` (1.80)** over `once_cell`/`lazy_static`, which are now legacy for these uses.

```rust
use std::sync::LazyLock;
use regex::Regex;
// Compiled once, on first use; thread-safe; no macro.
static EMAIL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[^@]+@[^@]+$").unwrap());
```

`parking_lot`'s `Mutex`/`RwLock` are smaller and faster and never poison, but std's locks are now competitive and poisoning surfaces bugs — reach for `parking_lot` only when profiling justifies it. Use `dashmap` for a sharded concurrent map when a single `RwLock<HashMap>` is a contention bottleneck.

## Concurrency: threads, scopes, channels, atomics

`std::thread::scope` (1.63) lets threads borrow local data without `'static` or `Arc`:

```rust
use std::thread;
fn parallel_sum(data: &[i64]) -> i64 {
    let mid = data.len() / 2;
    let (a, b) = data.split_at(mid);
    thread::scope(|s| {
        let h = s.spawn(|| a.iter().sum::<i64>());   // borrows `a`, no Arc needed
        let rb: i64 = b.iter().sum();
        h.join().unwrap() + rb
    })
}
```

Channels: `std::sync::mpsc` for simple cases; **crossbeam-channel** when you need `select!`, multiple consumers (mpmc), or better performance. For CPU-bound data parallelism reach for **rayon** — `par_iter()` turns a sequential pipeline parallel with one change:

```rust
use rayon::prelude::*;
let total: u64 = (0..1_000_000u64).into_par_iter().map(expensive).sum();
```

`Send` means safe to move across threads; `Sync` means `&T` is safe to share. Both are auto-derived. Atomics need an ordering: use `Relaxed` for counters, `Acquire`/`Release` to publish/consume data through a flag, `SeqCst` only when you need a single global order and can't reason about less. Validate lock-free code with `loom`.

## Async Rust with Tokio

Use **Tokio (1.53)** as the runtime. Async fn in traits (1.75) works for static dispatch; you do **not** need `async-trait` for that anymore. But it is **not dyn-compatible**: for `dyn` async traits (trait objects) you still need the **async-trait (0.1.91)** crate, and there is no stable way to bound the returned future as `Send` at the trait level via Return Type Notation (still nightly as of 1.98 — the stabilization PR was closed unmerged in December 2025, blocked on the next-generation trait solver) — use the `trait-variant` crate or a manual `impl Future + Send` return type.

```rust
#[tokio::main]                       // sets up the multi-thread runtime
async fn main() -> anyhow::Result<()> {
    let body = reqwest::get("https://example.com").await?.text().await?;
    println!("{} bytes", body.len());
    Ok(())
}

// Native async fn in a trait: fine for static dispatch (generic bounds).
trait Fetcher {
    async fn fetch(&self, url: &str) -> anyhow::Result<String>;
}

// Need `dyn Fetcher`? Use async-trait, which boxes the returned future.
#[async_trait::async_trait]
trait DynFetcher {
    async fn fetch(&self, url: &str) -> anyhow::Result<String>;
}

// Need the returned future to be Send at the trait level? Generate a Send variant:
#[trait_variant::make(SendFetcher: Send)]
trait LocalFetcher {
    async fn fetch(&self, url: &str) -> anyhow::Result<String>;
}
```

Structured concurrency with `JoinSet`; offload blocking work with `spawn_blocking`:

```rust
use tokio::task::JoinSet;
async fn fetch_all(urls: Vec<String>) -> Vec<anyhow::Result<String>> {
    let mut set = JoinSet::new();
    for url in urls {
        set.spawn(async move { reqwest::get(&url).await?.text().await.map_err(Into::into) });
    }
    let mut out = Vec::new();
    while let Some(res) = set.join_next().await {
        out.push(res.expect("task panicked"));
    }
    out
}

// CPU-bound / blocking work must not run on the async runtime threads.
let hash = tokio::task::spawn_blocking(move || expensive_cpu_hash(&data)).await?;
```

`select!` gotchas: **every branch's future must be cancellation-safe**, because losing branches are dropped mid-poll. Do not hold state you need across a `select!` in a non-cancel-safe future; move it out or use a `tokio::select!` with `biased;` when order matters. Don't `.await` while holding a `std::sync::Mutex` guard across the await point — use `tokio::sync::Mutex` only when the lock must be held across `.await`, otherwise prefer a std lock in a tight non-async scope.

For streams, use `tokio_stream::StreamExt` / `futures::StreamExt`. `Pin` matters when you store or poll futures manually; most code never touches it directly thanks to `async`/`await`.

## Serde in depth

**serde 1.0.229** with `serde_json 1.0.151`. `rustc-serialize` is dead. Use container and field attributes to match wire formats without renaming your Rust fields:

```rust
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct User {
    pub id: u64,
    pub display_name: String,
    #[serde(default)]                          // missing -> Default
    pub roles: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}
```

`deny_unknown_fields` catches typos and schema drift — use it for config and internal APIs. `#[serde(flatten)]` is convenient but disables `deny_unknown_fields` and costs an intermediate buffer; avoid it on hot paths. Zero-copy deserialization borrows from the input with a lifetime:

```rust
#[derive(Deserialize)]
struct Borrowed<'a> {
    #[serde(borrow)]
    name: &'a str,     // points into the source buffer; no allocation
}
```

For lazy/streaming JSON, `serde_json::value::RawValue` defers parsing a subtree; `serde_json::Value` is the dynamic fallback when the shape is unknown. Custom (de)serialization uses `#[serde(with = "module")]` or `serialize_with`/`deserialize_with`.

## Unsafe Rust, FFI, and `MaybeUninit`

Keep `unsafe` small, wrapped in safe abstractions, and always documented with a `// SAFETY:` comment justifying each invariant. Edition 2024 requires explicit `unsafe {}` inside `unsafe fn`.

```rust
// SAFETY comments are mandatory and specific.
pub fn split_first_mut<T>(s: &mut [T]) -> Option<(&mut T, &mut [T])> {
    if s.is_empty() { return None; }
    let ptr = s.as_mut_ptr();
    // SAFETY: s is non-empty, so index 0 is valid; the two resulting references
    // are disjoint (index 0 vs indices 1..len), so no aliasing occurs.
    unsafe { Some((&mut *ptr, std::slice::from_raw_parts_mut(ptr.add(1), s.len() - 1))) }
}
```

`unsafe extern` blocks with per-item safety qualifiers (edition 2024):

```rust
unsafe extern "C" {
    pub safe fn abs(input: i32) -> i32;                 // callable without unsafe
    pub unsafe fn strlen(p: *const std::ffi::c_char) -> usize;  // needs unsafe at call site
}
```

Use `MaybeUninit<T>` for deferred initialization, `NonNull<T>` for non-null raw pointers in data structures. For plain-old-data reinterpretation prefer **bytemuck** (safe `Pod`/`Zeroable` casts) or **zerocopy** over hand-written `transmute`. Run `cargo +nightly miri test` to catch UB (aliasing violations, uninitialized reads, out-of-bounds) in code exercising `unsafe`. For FFI bindings: **bindgen** (C headers → Rust), **cxx** (safe C++ interop), **pyo3** (Python), **wasm-bindgen** (JS/Wasm).

## Macros

Reach for `macro_rules!` for repetitive patterns; write a proc macro only when you need to inspect or generate types/derives. `macro_rules!` is hygienic for local variables. Key fragment specifiers: `expr`, `ident`, `ty`, `pat`, `path`, `tt`, `literal`, `block`, `stmt`.

```rust
macro_rules! hashmap {
    ($($k:expr => $v:expr),* $(,)?) => {{
        let mut m = std::collections::HashMap::new();
        $( m.insert($k, $v); )*
        m
    }};
}
let scores = hashmap!{ "a" => 1, "b" => 2 };
```

Proc macros live in a separate `proc-macro = true` crate, built on **syn**, **quote**, and **proc-macro2**; use **darling** to parse attribute arguments ergonomically. Inspect expansions with `cargo expand`.

## Modules, visibility, and semver

Default to private; expose deliberately with `pub`, `pub(crate)`, `pub(super)`. Re-export a flat public API from the crate root with `pub use`. Features must be **additive** — enabling one never removes items or changes behavior; gate optional deps with `dep:` and code with `#[cfg(feature = "...")]`. Mark extensible public enums/structs `#[non_exhaustive]`. Verify you didn't break semver before publishing with **cargo-semver-checks (0.48)**.

```rust
// src/lib.rs — curate the public surface.
mod client;
mod error;
pub use client::Client;
pub use error::StoreError;

#[cfg(feature = "postgres")]
pub mod postgres;
```

## Testing

Unit tests live in-module behind `#[cfg(test)]`; integration tests in `tests/` see only the public API; doc tests keep examples honest. Use **cargo-nextest** as the runner (faster, per-test process isolation) — note it does not run doc tests, so run those separately.

```rust
pub fn add(a: i32, b: i32) -> i32 { a + b }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds() { assert_eq!(add(2, 2), 4); }

    #[test]
    #[should_panic(expected = "overflow")]
    fn detects_overflow() { let _ = i32::MAX.checked_add(1).expect("overflow"); }
}

/// Doubles a number.
/// ```
/// assert_eq!(mycrate::double(21), 42);
/// ```
pub fn double(n: i32) -> i32 { n * 2 }
```

Tooling per job: **insta** for snapshot testing (`cargo insta review`); **proptest (1.11)** for property-based testing; **mockall** for mocking traits; **criterion (0.8)** or **divan** for benchmarks; **testcontainers** for ephemeral DBs in integration tests; **cargo-mutants** for mutation testing; **cargo-llvm-cov** for coverage (prefer over the older tarpaulin).

```rust
proptest::proptest! {
    #[test]
    fn roundtrip(s in ".*") {
        proptest::prop_assert_eq!(decode(&encode(&s)), s);
    }
}
```

```rust
// benches/bench.rs — Criterion; declare [[bench]] harness = false in Cargo.toml.
use criterion::{criterion_group, criterion_main, Criterion, black_box};
fn bench_add(c: &mut Criterion) {
    c.bench_function("add", |b| b.iter(|| add(black_box(2), black_box(2))));
}
criterion_group!(benches, bench_add);
criterion_main!(benches);
```

## Performance practice

Measure before optimizing (`cargo flamegraph`, `criterion`, `divan`). Then: avoid needless allocation (reuse buffers, `with_capacity`, borrow); use **smallvec** for collections that are usually tiny; `Box<[T]>` over `Vec<T>` for fixed-size owned slices to save a word and signal immutability. `#[inline]` only across crate boundaries for small hot functions — within a crate the compiler already inlines well. Monomorphization is fast but grows binary and compile time; `dyn` trades a virtual call for smaller code. Tune release builds for shipping binaries:

```toml
[profile.release]
opt-level = 3
lto = "thin"            # "fat" for max cross-crate inlining at higher link cost
codegen-units = 1       # better optimization, slower compile
panic = "abort"         # smaller/faster if you don't unwind; changes panic semantics
strip = "debuginfo"

[profile.dev]
opt-level = 0
debug = true

# Optimize one hot dependency even in dev:
[profile.dev.package.image]
opt-level = 3
```

PGO and BOLT give further gains for CPU-bound binaries but add build complexity — reserve for measured hot paths.

## Toolchain and tooling configuration

Pin the toolchain for reproducibility:

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.98"
components = ["rustfmt", "clippy", "rust-analyzer"]
```

`rustfmt.toml` — **stable keys only**. `imports_granularity`, `group_imports`, `wrap_comments`, `format_code_in_doc_comments`, and `normalize_comments` are **nightly-gated**; do not put them in a config used with stable `cargo fmt` or every run prints warnings. Stable keys:

```toml
# rustfmt.toml
edition = "2024"
max_width = 100
hard_tabs = false
newline_style = "Unix"
use_field_init_shorthand = true
use_try_shorthand = true
reorder_imports = true
```

Clippy config via the Cargo `[lints]` table (the `priority = -1` on a group lets later single-lint overrides win):

```toml
# Cargo.toml
[lints.rust]
unsafe_op_in_unsafe_fn = "warn"

[lints.clippy]
pedantic = { level = "warn", priority = -1 }
# Targeted overrides win because they have the default priority 0:
module_name_repetitions = "allow"
missing_errors_doc = "allow"
unwrap_used = "deny"
```

Guidance on lint groups: enable **`clippy::pedantic`** (opinionated, occasional false positives — `allow` the noisy ones); consider **`clippy::nursery`** (in-development lints) and **`clippy::cargo`** selectively. **Never enable `clippy::restriction` as a group** — Clippy itself warns if you try, and its lints can contradict each other; cherry-pick individual lints from it (`unwrap_used`, `dbg_macro`, `undocumented_unsafe_blocks`). `clippy.toml` holds lint-specific thresholds (e.g. `msrv = "1.98"`, `too-many-arguments-threshold = 8`).

`.cargo/config.toml` for a faster linker and handy aliases (local/CI only, not published):

```toml
# .cargo/config.toml
[target.x86_64-unknown-linux-gnu]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=mold"]   # mold/lld: dramatically faster link times

[alias]
lint = "clippy --all-targets --all-features -- -D warnings"
```

Supply-chain and CI hygiene: **cargo-deny** (`deny.toml` for licenses/advisories/bans/sources), **cargo-audit** (RustSec advisories), **cargo-machete** or **cargo-udeps** (unused deps), **cargo-hakari** (workspace-hack for faster workspace builds), **cargo-msrv** (find/verify MSRV), **cargo fuzz** (libFuzzer). Use **bacon** for a fast background check loop (the current answer over `cargo-watch`); **sccache** to cache compilation across builds.

Canonical CI command set:

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
cargo test --doc                # nextest doesn't run doc tests
cargo deny check
```

## Ecosystem quick reference

| Job | Use | Avoid / superseded |
|-----|-----|--------------------|
| Error (library) | thiserror 2.x | failure, error-chain, quick-error (dead) |
| Error (app) | anyhow 1.x | `Box<dyn Error>` in public APIs |
| Serialization | serde 1.x + serde_json | rustc-serialize (dead) |
| Async runtime | tokio 1.x | async-std (discontinued); smol/glommio (niche) |
| HTTP client | reqwest 0.13 | surf/isahc (stale); hyper (lower-level) |
| Web framework | axum 0.8 | tide (discontinued); warp (superseded); actix-web/rocket (alternatives) |
| CLI | clap 4.x (derive) | structopt (merged into clap, deprecated) |
| Logging/tracing | tracing + tracing-subscriber | slog; log/env_logger for simple bins |
| Date/time | jiff 0.2 (new work); chrono 0.4 (soft-deprecated — its author now recommends jiff in chronotope/chrono #1768) | time is fine for formatting/simple needs |
| RNG | rand 0.9+ (`rng()`, `random_range()`) | rand 0.8 API (`thread_rng`, `gen_range`) |
| Concurrent map | dashmap | — |
| Locks | std; parking_lot when profiled | — |
| Lazy statics | std OnceLock/LazyLock | once_cell, lazy_static (legacy) |
| Hashing | std default; foldhash/ahash for speed | fnv (older) |
| Data parallelism | rayon 1.x | manual thread pools |
| Test runner | cargo-nextest | — |
| Snapshot / property / mock | insta / proptest / mockall | — |
| Benchmarking | criterion 0.8 / divan | unstable `#[bench]` (removed) |
| ORM / SQL | sqlx 0.8 (SQL-first); sea-orm 2.0 (ActiveRecord); diesel 2.x (compile-time DSL, sync) | — |
| FFI | bindgen / cxx / pyo3 / wasm-bindgen | — |
| UB / concurrency checking | miri / loom | — |

## Anti-patterns to avoid

- **`.unwrap()`/`.clone()` as borrow-checker escape hatches.** Restructure ownership or use `expect` with a reason; clone only when you genuinely need a second owned value.
- **`Arc<Mutex<T>>` as a default.** Prefer passing `&mut T`, message-passing channels, or `rayon`. Reach for shared mutable state only when the design truly requires it.
- **Reflexively adding `async-trait`.** Native `async fn` in traits (1.75) covers static dispatch; only use `async-trait` for `dyn` trait objects.
- **`once_cell`/`lazy_static`/`chrono`/`structopt` in new code.** Use std `LazyLock`/`OnceLock`, `jiff`, and clap 4 derive.
- **Nightly rustfmt keys on stable** (`imports_granularity`, `group_imports`, `wrap_comments`) — they only warn and do nothing.
- **`Box<dyn Error>` in a library's public API** — it erases the error type so callers can't match; return a `thiserror` enum.
- **`#![allow(clippy::all)]` or enabling `clippy::restriction` wholesale** — silences real bugs / contradicts itself. Configure lints in the `[lints]` table and cherry-pick restriction lints.
- **Blocking calls (`std::fs`, `std::thread::sleep`, heavy CPU) inside async tasks** — starves the runtime; use `tokio::fs`, `tokio::time::sleep`, or `spawn_blocking`.
- **Holding a lock guard across `.await`** — deadlock/contention risk; drop the guard first or use `tokio::sync::Mutex` deliberately.
- **`#[serde(flatten)]` on hot paths** — disables `deny_unknown_fields` and buffers; model the fields explicitly.

- **Research date:** 22 August 2026
- **Research basis:** current official docs, release notes, specifications, changelogs, and primary repositories.
