---
type: "agent_requested"
description: "Rust 2024 edition coding guidelines"
---

# Comprehensive Rust 2024 edition coding guidelines for the bleeding edge

**Rust's 2024 edition, stabilized in version 1.85 (February 2025), represents the largest edition release to date**, unlocking async closures, revised lifetime capture rules, stricter unsafe semantics, and a modernized Cargo resolver. Nine subsequent releases through 1.93 (January 2026) have continued this momentum, stabilizing let chains, trait object upcasting, safe `std::arch` intrinsics, file locking, `lld` as the default linker, and dozens of new standard library APIs. This guide documents every significant feature from 1.85 through 1.93+, provides modern idioms that fully leverage the 2024 edition, and tracks the nightly features and project goals shaping Rust's 2026 trajectory. The overarching philosophy: **greenfield, no MSRV constraints, newest patterns first**.

---

## The 2024 edition rewrites the rules

The 2024 edition (`edition = "2024"` in Cargo.toml, implying `resolver = "3"`) makes sweeping changes across lifetime semantics, unsafe ergonomics, macros, and the standard prelude. Every project targeting the latest toolchain should adopt it unconditionally.

### Lifetime capture and RPIT overhaul

The single most impactful language change is **automatic lifetime capture in return-position `impl Trait`** (RPIT). In prior editions, RPIT in bare functions only captured type parameters that appeared syntactically in the bounds—a source of confusing errors. In 2024, all in-scope generic parameters including lifetimes are captured by default:

```rust
// 2024 edition: 'a is implicitly captured — just works
fn chars<'a>(s: &'a str) -> impl Iterator<Item = char> {
    s.chars()
}
```

When you need to *exclude* a lifetime from capture, use the **precise capturing syntax** `use<..>` (stabilized in 1.82, essential in 2024):

```rust
fn f<'a, T>(x: &'a (), y: T) -> impl Sized + use<T> {
    // Captures only T, not 'a
    y
}
```

### Stricter unsafe semantics

Three changes tighten unsafe code:

- **`unsafe_op_in_unsafe_fn`** is now warn-by-default. Every unsafe operation inside an `unsafe fn` must be wrapped in an explicit `unsafe {}` block with a `// SAFETY:` comment.
- **`unsafe extern` blocks** are required—bare `extern "C" { }` is a compile error.
- **Unsafe attributes** like `#[no_mangle]` must be written as `#[unsafe(no_mangle)]`.

```rust
// 2024 edition pattern:
unsafe fn read_val<T>(ptr: *const T) -> T {
    // SAFETY: caller guarantees ptr is valid and aligned
    unsafe { ptr.read() }
}

#[unsafe(no_mangle)]
pub extern "C" fn ffi_entry(x: i32) -> i32 { x + 1 }
```

### Other edition changes at a glance

**`gen` keyword reserved** for future generator blocks. **`expr` fragment specifier** in `macro_rules!` now matches `const {}` blocks and `_` expressions (use `expr_2021` for the old behavior). **`Future` and `IntoFuture`** join the prelude. **`Box<[T]>`** implements `IntoIterator`. **`std::env::set_var`** and `remove_var` are now unsafe. **Cargo resolver v3** (MSRV-aware) is the default. **Rustfmt** supports an independent `style_edition = "2024"`.

---

## Release-by-release stabilization from 1.85 to 1.93

Each six-week release has delivered meaningful features. The table below captures the most important stabilizations; subsequent sections reference these by version number.

| Version | Date | Headline features |
|---------|------|-------------------|
| **1.85** | Feb 2025 | 2024 edition, **async closures** (`async \|\| {}`), `#[diagnostic::do_not_recommend]` |
| **1.86** | Apr 2025 | **Trait object upcasting** (`&dyn Trait` → `&dyn Supertrait`), `#[target_feature]` on safe functions, `missing_abi` warn |
| **1.87** | May 2025 | `asm_goto`, **`precise_capturing_in_traits`** (`use<..>` in trait RPIT), `io::pipe()`, safe `std::arch` intrinsics, `Vec::extract_if` |
| **1.88** | Jun 2025 | **`let` chains** in `if`/`while` (2024 only), naked functions (`#[unsafe(naked)]`), `cfg(true)`/`cfg(false)`, `Cell::update`, Cargo auto-GC |
| **1.89** | Aug 2025 | `generic_arg_infer` (`[0; _]`), `#[repr(u128)]`/`#[repr(i128)]`, AVX-512 stabilized, **`File::lock`** API, `mismatched_lifetime_syntaxes` lint, `Result::flatten` |
| **1.90** | Sep 2025 | **`lld` default linker** on x86_64-linux, multi-package `cargo publish`, const float ops |
| **1.91** | Oct 2025 | `strict_*` integer ops, `Path::file_prefix`, `Duration::from_hours`/`from_mins`, `core::iter::chain`, **`build.build-dir`** in Cargo, aarch64-windows → Tier 1 |
| **1.92** | Dec 2025 | **`RwLockWriteGuard::downgrade`**, `Box/Rc/Arc::new_zeroed`, never-type lints deny-by-default |
| **1.93** | Jan 2026 | `asm_cfg`, **`fmt::from_fn`**, `MaybeUninit::assume_init_ref`/`assume_init_mut`, `Vec/String::into_raw_parts`, `VecDeque::pop_front_if`/`pop_back_if` |

### Critical new lints across releases

| Lint | Level | Version | Purpose |
|------|-------|---------|---------|
| `unsafe_op_in_unsafe_fn` | warn | 1.85 (2024 ed.) | Require explicit `unsafe {}` inside `unsafe fn` |
| `missing_abi` | warn | 1.86 | Flag `extern` without explicit ABI |
| `dangerous_implicit_autorefs` | deny | 1.89 | Prevent implicit autoref of raw pointer derefs |
| `mismatched_lifetime_syntaxes` | warn | 1.89 | Inconsistent lifetime syntax between inputs/outputs |
| `never_type_fallback_flowing_into_unsafe` | deny | 1.92 | Guard against unsound never-type coercions |
| `deref_nullptr` | deny | 1.93 | Prevent null pointer dereferences |
| `const_item_interior_mutations` | warn | 1.93 | Warn against mutating interior-mutable const items |

---

## Ownership, lifetimes, and the type system in 2024

### Interior mutability hierarchy

With `LazyLock` (1.80) and `OnceLock` (1.70) in std, **the `lazy_static!` and `once_cell` crates are fully superseded**:

```rust
use std::sync::LazyLock;

// Replaces lazy_static! — the modern way to declare statics
static GLOBAL: LazyLock<HashMap<&str, u32>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert("key", 42);
    m
});

// OnceLock for runtime-initialized statics
use std::sync::OnceLock;
static CONFIG: OnceLock<String> = OnceLock::new();
fn config() -> &'static str {
    CONFIG.get_or_init(|| std::env::var("CONFIG").unwrap_or("default".into()))
}
```

`DerefMut` for `LazyCell`/`LazyLock` was stabilized in 1.89, enabling in-place mutation of lazy values.

### Generic associated types unlock lending patterns

GATs (stable since 1.65) enable lifetime-parameterized associated types—the foundation for lending iterators, zero-copy parsers, and database cursors:

```rust
trait LendingIterator {
    type Item<'a> where Self: 'a;
    fn next<'a>(&'a mut self) -> Option<Self::Item<'a>>;
}
```

Full exploitation of lending iterators awaits the **Polonius borrow checker** (alpha on nightly, targeting 2026 stabilization), which accepts conditional borrow patterns that the current NLL checker rejects.

### Const generics and compile-time evaluation

**`generic_arg_infer`** (1.89) lets you write `[0u8; _]` and `Foo::<_>` where the compiler infers const arguments. Basic const generics (`const N: usize`) remain the stable foundation. The `min_generic_const_args` project—replacing the broken `generic_const_exprs`—is being prototyped for 2026 stabilization.

**`const fn` capabilities** expanded dramatically in 1.83, allowing mutable references, raw pointers, and float operations in const contexts. **`const` blocks** (1.79) force compile-time evaluation inline:

```rust
const { assert!(std::mem::size_of::<MyStruct>() <= 64) };
```

### The never type approaches full stabilization

The 2024 edition changed never-type fallback from `()` to `!`. Rust 1.92 elevated the related lints to deny-by-default, affecting ~500 crates. The stabilization plan proceeds: once 2024 adoption is widespread, `!` becomes the fallback in all editions, `Infallible` becomes `= !`, and `!` stabilizes as a first-class type.

---

## Async and concurrency patterns for the latest toolchain

### Async closures replace the `|| async {}` workaround

Stabilized in **1.85**, async closures (`async || {}`) solve the longstanding inability to borrow captured variables across await points:

```rust
let mut vec: Vec<String> = vec![];
let closure = async || {
    vec.push(std::future::ready(String::from("hello")).await);
};
```

Three new traits—`AsyncFn`, `AsyncFnMut`, `AsyncFnOnce`—are in the prelude across all editions. Use `impl AsyncFn(Args) -> Output` as bounds.

### Async fn in traits without the proc macro

Native async fn in traits has been stable since 1.75, making the **`#[async_trait]` proc macro obsolete for static dispatch**. The remaining gap is `Send` bounds—solved today by the `trait_variant` crate and soon by **return-type notation** (RTN), whose stabilization PR was filed in early 2025:

```rust
// trait_variant: creates a Send-bound variant automatically
#[trait_variant::make(SendService: Send)]
trait Service {
    async fn call(&self, req: Request) -> Response;
}

// RTN (approaching stabilization): per-method Send bounds
fn spawn<S: Service<call(..): Send> + Send + 'static>(s: S) {
    tokio::spawn(async move { s.call(req).await });
}
```

### The async runtime landscape has consolidated

**Tokio** (latest 1.49) is the unambiguous default for applications, powering axum, reqwest, tonic, and sqlx. **`async-std` was discontinued in March 2025**—projects should migrate to Tokio or `smol`. `smol` remains a lightweight alternative for library authors seeking runtime agnosticism.

### Structured concurrency with JoinSet and CancellationToken

```rust
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

let token = CancellationToken::new();
let mut set = JoinSet::new();
for i in 0..10 {
    let t = token.clone();
    set.spawn(async move {
        tokio::select! {
            _ = t.cancelled() => None,
            result = compute(i) => Some(result),
        }
    });
}
// All tasks complete or are cancelled when `set` is dropped
```

### Sync concurrency primitives

**`RwLockWriteGuard::downgrade`** (1.92) enables downgrading a write lock to a read lock without releasing it—eliminating a race condition window. `File::lock` and friends (1.89) bring cross-platform advisory file locking to std.

For concurrent collections: **`papaya`** provides lock-free reads ideal for read-heavy async workloads (no deadlock risk, `Send + Sync` guards). **`dashmap`** offers the simplest API for write-heavy patterns. Both supersede hand-rolled `RwLock<HashMap>`.

### Generators and `iter!` are on the horizon

The `gen` keyword was reserved in 2024 edition. The `iter!` macro landed experimentally on nightly (tracking: #142269) but requires an RFC before stabilization:

```rust
// Nightly only
#![feature(iter_macro)]
let fib = std::iter::iter! {
    let (mut a, mut b) = (0u64, 1);
    loop { yield a; (a, b) = (b, a + b); }
};
```

On stable, `genawaiter` provides a similar experience. The `async-stream` crate offers `stream!` for async generators.

---

## Error handling: thiserror 2.x and the modern stack

### The standard pairing

**`thiserror` 2.x** (November 2024) for library error types and **`anyhow`** (or `color-eyre`) for application error propagation remains the canonical pattern:

```rust
// Library errors: structured, matchable
#[derive(thiserror::Error, Debug)]
pub enum StorageError {
    #[error("entity not found: {0}")]
    NotFound(Uuid),
    #[error("connection failed")]
    Connection(#[from] io::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

// Application errors: context-rich, propagated with ?
use anyhow::{Context, Result};
fn load_config(path: &str) -> Result<Config> {
    let raw = std::fs::read_to_string(path)
        .context(format!("failed to read {path}"))?;
    toml::from_str(&raw).context("invalid config format")
}
```

**thiserror 2.x breaking changes**: raw identifier format strings (`{r#type}` → `{type}`), `r#source` escape for fields named "source", and no_std support via `default-features = false`.

### Diagnostic-quality errors with miette

For CLI tools and compilers needing source-code-annotated error output, **`miette`** (7.6) composes with thiserror:

```rust
#[derive(Debug, thiserror::Error, miette::Diagnostic)]
#[error("invalid syntax")]
#[diagnostic(code(parser::syntax), help("check your brackets"))]
struct ParseError {
    #[source_code] src: String,
    #[label("unexpected token here")] span: miette::SourceSpan,
}
```

### Compiler diagnostic attributes

**`#[diagnostic::on_unimplemented]`** (1.78) and **`#[diagnostic::do_not_recommend]`** (1.85) let library authors dramatically improve trait error messages—used by Axum, Bevy, and Diesel in production:

```rust
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot serve as an HTTP handler",
    note = "See https://docs.rs/axum/latest/axum/handler"
)]
pub trait Handler<T> { /* ... */ }
```

---

## Cargo, testing, and project architecture

### Modern Cargo.toml for 2024 edition

```toml
[workspace]
resolver = "3"           # Required explicitly for virtual workspaces
members = ["crates/*"]

[workspace.package]
edition = "2024"
rust-version = "1.85.0"

[workspace.lints.rust]
unsafe_code = "deny"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
thiserror = "2"
```

**Cargo resolver v3** (default with 2024 edition) prefers dependency versions whose `rust-version` is compatible with yours. **Cargo auto-GC** (1.88) cleans cached `.crate` files after 3 months. **`build.build-dir`** (1.91) enables configurable intermediate artifact directories. **Multi-package publishing** (`cargo publish --workspace`) landed in 1.90.

### Build performance: lld, sccache, and Cranelift

**`lld` became the default linker on `x86_64-unknown-linux-gnu` in Rust 1.90**, delivering up to **7× faster linking** for incremental builds. For other platforms, configure in `.cargo/config.toml`:

```toml
[build]
rustflags = ["-C", "link-arg=-fuse-ld=lld"]
```

**`sccache`** provides shared compilation caching across CI and local builds. The **Cranelift backend** (nightly) trades runtime performance for dramatically faster dev builds.

### Production release profile

```toml
[profile.release]
opt-level = 3
lto = "fat"
codegen-units = 1
strip = "symbols"
panic = "abort"

[profile.dev.package."*"]
opt-level = 2    # Optimize deps in dev builds
```

### Testing with cargo-nextest and insta

**`cargo-nextest`** (0.9.126) runs each test in its own process for isolation, provides structured output, retry support, and JUnit XML for CI. **Combined doctests** (1.85) compile all doctests into a single binary—the Jiff crate reported speedup from **12.56s → 0.25s** for 903 doctests.

**`insta`** (1.43) for snapshot testing with `cargo-insta` review workflow:

```rust
#[test]
fn test_output() {
    let response = get_response();
    insta::assert_yaml_snapshot!(response, {
        ".timestamp" => "[timestamp]",  // redact non-deterministic fields
        ".id" => "[uuid]",
    });
}
```

**Benchmarking**: `divan` for simplicity and built-in allocation profiling; `criterion` (0.6) for statistical rigor and HTML reports. Both require `harness = false`.

### CI pipeline skeleton

```yaml
- uses: dtolnay/rust-toolchain@stable
  with: { components: "rustfmt, clippy" }
- run: cargo fmt --all -- --check
- run: cargo clippy --workspace --all-targets -- -D warnings
- run: cargo nextest run --workspace --profile ci
- run: cargo test --doc
- uses: EmbarkStudios/cargo-deny-action@v2
- run: cargo llvm-cov nextest --workspace --lcov --output-path lcov.info
```

**`release-plz`** automates release PRs, changelog generation via `git-cliff`, semver checking via `cargo-semver-checks`, and crates.io publishing on merge.

---

## Performance, memory, and macros

### Optimization profiles and PGO

**Profile-guided optimization** via `cargo-pgo` provides ~10%+ improvement by using runtime profiling data. Combined with `lto = "fat"`, this yields maximum throughput. For production binaries: instrument → run representative workload → rebuild with profiles.

**SIMD**: `std::arch` intrinsics are now **safe to call** from functions with matching target features (1.87). **Portable SIMD** (`std::simd`) remains nightly-only; on stable, use the `wide` or `pulp` crates.

### Data structures and hashing

Rust's `HashMap` is backed by **hashbrown** (SwissTable). The hashbrown crate now defaults to **`foldhash`**—~2× faster than SipHash with a smaller state (40 bytes vs 48 for std). For direct use:

```rust
use foldhash::HashMap; // or hashbrown::HashMap with foldhash
let mut map = HashMap::new();
map.insert(42, "fast");
```

For inline storage: **`SmallVec<[T; N]>`** for stack-allocated small vectors with heap fallback, **`CompactString`** for strings ≤24 bytes stored inline (same size as `String`), **`SmolStr`** for immutable frequently-cloned strings with O(1) clone.

### Macro changes in 2024 edition

The `expr` fragment specifier now matches `const {}` blocks and `_` expressions. Use `expr_2021` for backward compatibility. Migration: `cargo fix --edition` auto-converts where needed. The `gen` keyword is reserved. `missing_fragment_specifier` is a hard error.

**Procedural macros** use `syn` 2.0 + `proc-macro2` 1.0 + `quote` 1.0. **Declarative derive macros** (RFCs accepted, implementation underway) will eventually reduce the need for proc-macro crates, cutting compile times.

---

## API design, builders, and code style

### The `bon` builder pattern

**`bon`** (3.x) is the recommended builder derive—used by the crates.io backend, with compile-time checked typestate builders that work on both structs and functions:

```rust
use bon::Builder;

#[derive(Builder)]
struct Config {
    #[builder(into)] host: String,
    #[builder(default = 8080)] port: u16,
    tls: Option<TlsConfig>,  // automatically optional
}

let cfg = Config::builder()
    .host("localhost")
    .port(443)
    .tls(tls_config)
    .build();
```

`bon` compiles **36% faster** than `typed-builder` and produces human-readable typestate signatures. It leverages `#[diagnostic::on_unimplemented]` for clear error messages.

### Key API guidelines

Per the official Rust API Guidelines: implement `From` (never `Into` directly), use `as_`/`to_`/`into_` conversion naming, eagerly derive `Clone`, `Debug`, `PartialEq`, `Eq`, `Hash`, and `Default`. **Only smart pointers should implement `Deref`**—using it for newtypes is an anti-pattern. Use `#[non_exhaustive]` on public enums and structs for semver safety.

### Clippy and rustfmt configuration

```toml
# Cargo.toml — workspace lints
[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
unwrap_used = "warn"
expect_used = "warn"
enum_glob_use = "deny"
```

```toml
# rustfmt.toml
style_edition = "2024"
edition = "2024"
max_width = 100
```

---

## The ecosystem stack for 2026

### Web: axum + tower + tokio

**Axum 0.8** (Tokio team) is the dominant web framework—macro-free routing, extractor-based request parsing, and shared `tower` middleware with hyper 1.x and tonic:

```rust
use axum::{extract::{Path, State, Json}, routing::get, Router};

async fn get_user(
    Path(id): Path<u64>,
    State(pool): State<PgPool>,
) -> Json<User> {
    let user = sqlx::query_as!(User, "SELECT * FROM users WHERE id = $1", id as i64)
        .fetch_one(&pool).await.unwrap();
    Json(user)
}

let app = Router::new()
    .route("/users/{id}", get(get_user))
    .with_state(pool);
```

**`tower-http`** (0.6) provides CORS, compression (gzip/brotli/zstd), tracing, rate limiting, and more—all composable as layers.

### Serialization beyond JSON

For binary serialization: **`bitcode`** (0.6) produces the smallest output with excellent speed (100% safe Rust). **`rkyv`** (0.8) enables zero-copy deserialization—read structured data directly from byte buffers without parsing. **`bincode`** 2.0 is the general-purpose choice with its own `Encode`/`Decode` traits.

### Database access

**`sqlx`** (0.8) for compile-time checked SQL with async native drivers. **Diesel** (2.2) for the strongest compile-time type safety via Rust's type system. **SeaORM** (1.x) for Active Record productivity with async-first design. All three are production-grade; sqlx is the most common choice for async applications.

### Observability with tracing

**`tracing`** (0.1.41) is the standard instrumentation library, replacing `log` for modern projects:

```rust
#[tracing::instrument(fields(user_id = %id), skip(pool), ret, err)]
async fn get_user(id: u64, pool: &PgPool) -> Result<User, Error> {
    tracing::info!("fetching user");
    // ...
}
```

Compose layers via `tracing-subscriber`: `EnvFilter` for log-level filtering, `fmt::Layer` for output formatting, `tracing-opentelemetry` for distributed tracing export, and `console-subscriber` for the tokio-console debugger.

### Recommended crate versions (early 2026)

| Category | Crate | Version | Replaces |
|----------|-------|---------|----------|
| Async runtime | `tokio` | 1.49 | — |
| Web framework | `axum` | 0.8.x | — |
| HTTP client | `reqwest` | 0.14.3 | — |
| Serialization | `serde` + `serde_json` | 1.0.219 / 1.0.140 | — |
| Error (lib) | `thiserror` | **2.0.12** | thiserror 1.x |
| Error (app) | `color-eyre` | 0.6.x | — |
| Logging | `tracing` | 0.1.41 | `log` + `env_logger` |
| Database | `sqlx` | 0.8.x | — |
| CLI | `clap` | 4.5.x | structopt |
| Benchmark | `divan` / `criterion` | 0.1 / 0.6 | — |
| Builder | `bon` | 3.x | `typed-builder` |
| Statics | `std::sync::LazyLock` | 1.80+ | `lazy_static!`, `once_cell` |
| TLS | `rustls` | 0.23.x | OpenSSL bindings |
| Random | `rand` | 0.9 | rand 0.8 (`gen` → `random`) |

---

## Unsafe, embedded, WASM, and security

### Modern unsafe patterns

The 2024 edition's unsafe requirements compose with **strict provenance APIs** (stabilized 1.84): use `ptr.addr()`, `ptr.with_addr()`, and `ptr.map_addr()` instead of `as usize`/`as *const T` casts. For UB detection, **Miri** (accepted to POPL 2026) detects all de-facto UB in deterministic Rust programs—run `cargo +nightly miri test` on any code containing `unsafe`.

For C++ interop, **`cxx`** (1.0) provides compile-time verified safe bridges. For C, `bindgen` auto-generates FFI bindings from headers.

### Embedded Rust with Embassy

**Embassy** is the leading async embedded framework—no alloc, no heap, statically allocated tasks with automatic sleep (WFI/WFE when idle). HALs cover STM32, nRF, RP2040/RP2350, and ESP32. Combined with **`embedded-hal` 1.0** (stable, async traits on stable Rust 1.75+) and **`defmt`** for efficient deferred-formatting logging, embedded Rust is production-ready.

### WebAssembly targets

The `wasm32-wasi` target was renamed to **`wasm32-wasip1`**; the old name is removed in 1.91+. **`wasm32-wasip2`** (Tier 2 since 1.82) uses the Component Model with high-level WIT interfaces. For browser targets, **Leptos** (0.8, fine-grained reactivity) excels at web performance while **Dioxus** (0.7, React-like) targets cross-platform (web + desktop + mobile).

### Supply chain security pipeline

```bash
cargo audit          # RustSec vulnerability scanning
cargo deny check     # licenses + advisories + duplicate deps
cargo vet            # human audit tracking (imports Mozilla/Google audits)
```

**`cargo-auditable`** embeds the dependency tree into compiled binaries (<4kB overhead), enabling post-deployment vulnerability scanning—adopted by Alpine Linux, NixOS, and Chainguard.

---

## What's coming next: 2026 project goals and nightly features

The Rust project is shifting from 6-month to **annual goals** starting in 2026, with 45 proposed goals across eight flagship themes. The most impactful for daily development:

### Features approaching stabilization

| Feature | Status | Impact |
|---------|--------|--------|
| **Next-gen trait solver** (globally) | In production for coherence; full stabilization targeted 2026 | Fixes soundness bugs, enables TAIT, improves error messages |
| **Polonius borrow checker** | Alpha on nightly, targeting stabilizable form in 2026 | Lending iterators, conditional borrow patterns |
| **`cargo-script`** | RFC 3503 merged, implementation landed, blocking on rustdoc | Single-file Rust programs with inline dependencies |
| **Sized hierarchy** (Part I) | RFC 3729 implemented on nightly | Extern types, ARM SVE scalable vectors |
| **Public/private dependencies** | RFC 3516 accepted | Control which deps are part of your public API |
| **Cargo SBOM** | Active goal | Native software bill of materials generation |
| **Sanitizer stabilization** | ASan/LSan close; MSan/TSan need infrastructure | Production UB detection for FFI-heavy code |

### Experimental features to watch

**`iter!` macro** for sync generators (nightly, tracking #142269). **Pin ergonomics** with `&pin mut T` syntax and auto-reborrowing (active experiment). **Type-alias `impl Trait`** (TAIT) blocked on the new trait solver. **Ergonomic ref-counting** with a proposed `Alias`/`Share` trait for transparent `Arc` cloning in closures. **Declarative derive and attribute macros** (RFCs accepted)—will eliminate many proc-macro dependencies. **Async drop** for full structured concurrency (early experiment).

The "Beyond the `&`" theme aims to make user-defined smart pointers as ergonomic as native references through reborrow traits, field projections, and in-place initialization—all in active design work for 2026.

---

## Conclusion

Rust's 2024 edition and the nine releases through 1.93 have materially advanced the language's expressiveness and ergonomics. The most consequential changes for daily coding are **automatic RPIT lifetime capture** (eliminating a major class of confusing errors), **async closures** (closing the async/sync parity gap), **`let` chains** (cleaning up nested `if let` pyramids), and **`lld` as the default linker** (dramatically faster incremental builds). On the ecosystem side, the consolidation around tokio + axum + tower, thiserror 2.x + anyhow, and tracing provides a clear, well-integrated production stack.

The forward trajectory is equally significant. The new trait solver, Polonius, and Sized hierarchy will unlock patterns—lending iterators, extern types, scalable vectors—that currently require unsafe workarounds. Cargo-script will lower the barrier to entry. Pin ergonomics and generators will simplify async code. The key architectural insight for 2026: **design your trait boundaries and error types with the new solver in mind**, prefer `impl Trait` over `dyn Trait` where possible (dyn async trait support is still maturing), and invest in `tracing`-based observability from day one. The language is moving fast, and code written for the bleeding edge today aligns directly with where the ecosystem is heading.
