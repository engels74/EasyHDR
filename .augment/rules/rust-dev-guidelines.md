---
type: "agent_requested"
description: "Rust coding guidelines for 1.93+ and the 2024 edition"
---

# Comprehensive Rust coding guidelines for 1.93+ and the 2024 edition

**Rust's 2024 edition, stabilized in version 1.85 (February 2025), represents the largest edition transition yet** — reshaping lifetime capture rules, tightening unsafe semantics, reserving the `gen` keyword, and adding `Future`/`IntoFuture` to the prelude. Nine subsequent releases through 1.93 (January 2026) have delivered a steady stream of powerful features: async closures, trait object upcasting, `let` chains, naked functions, `lld` as default linker, and dozens of new APIs. This guide documents every modern pattern, stabilized feature, and ecosystem recommendation for greenfield Rust development targeting the latest stable toolchain — no MSRV constraints, no legacy workarounds. Each feature is annotated with its stabilization version to serve as a precise reference.

---

## The Rust 2024 edition and release-by-release feature map

The 2024 edition (Rust 1.85) introduced **15 language-level changes** that affect how code is written, compiled, and reasoned about. Understanding these changes is foundational to writing modern Rust.

### Core 2024 edition changes

**RPIT lifetime capture rules** are the most impactful change. In Rust 2024, `-> impl Trait` return types **implicitly capture all in-scope generic parameters including lifetimes**. Previously, only type parameters were captured, requiring explicit `+ '_` annotations for lifetimes. The new `use<..>` precise capturing syntax (stable since 1.82, extended to traits in 1.87) allows opting out:

```rust
// 2024 edition: 'a is implicitly captured
fn foo<'a>(x: &'a str) -> impl Display { x.len() }

// Opt out of capturing 'a when not needed
fn bar<'a, T: Sized>(x: &'a (), y: T) -> impl Sized + use<T> { y }
```

**`unsafe_op_in_unsafe_fn`** is warn-by-default in 2024 edition, requiring explicit `unsafe {}` blocks inside `unsafe fn` bodies. This separates the declaration that a function is unsafe to call from the permission to perform unsafe operations:

```rust
unsafe fn process_raw(ptr: *const u8, len: usize) -> &[u8] {
    // SAFETY: Caller guarantees ptr is valid for len bytes
    unsafe { std::slice::from_raw_parts(ptr, len) }
}
```

**`unsafe extern` blocks** require marking all `extern` blocks with `unsafe extern`, with individual items optionally marked `safe`:

```rust
unsafe extern "C" {
    pub safe fn sqrt(x: f64) -> f64;       // safe to call
    pub unsafe fn strlen(p: *const c_char) -> usize;  // unsafe
}
```

Additional 2024 edition changes include **`gen` keyword reservation** (for future generators), **never type fallback changes** (tightened coercion behavior), **`expr` macro fragment specifier** now matching `const` and `_` expressions (use `expr_2021` for old behavior), **temporary lifetime changes** (temporaries from block ends drop before locals — fixing lock deadlocks in `if let`), **`Future`/`IntoFuture` in prelude**, **`IntoIterator` for `Box<[T]>`**, **`env::set_var`/`remove_var` now unsafe**, **`#[unsafe(no_mangle)]`** attribute syntax, **resolver v3** as default, and **`rustfmt` style edition 2024** with version-sorting and improved formatting.

### Feature stabilization timeline: 1.85 through 1.93

| Version | Date | Headline Features |
|---------|------|-------------------|
| **1.85** | Feb 2025 | **2024 edition**, async closures (`async \|\| {}`), `#[diagnostic::do_not_recommend]`, `AsyncFn*` traits in prelude |
| **1.86** | Apr 2025 | **Trait object upcasting**, safe `#[target_feature]`, `Vec::pop_if`, `Once::wait` |
| **1.87** | May 2025 | **`asm_goto`**, `use<..>` in trait RPITIT, `Vec::extract_if`, anonymous pipes, `env::home_dir` un-deprecated |
| **1.88** | Jun 2025 | **`let` chains** (2024 ed.), **naked functions**, `cfg(true)`/`cfg(false)`, `Cell::update`, `HashMap::extract_if`, `dangerous_implicit_autorefs` lint |
| **1.89** | Aug 2025 | **Generic arg infer** (`[0; _]`), **`#[repr(u128/i128)]`**, `mismatched_lifetime_syntaxes` lint, `File::lock`/`unlock`, `Result::flatten`, AVX512 stabilization |
| **1.90** | Sep 2025 | **`lld` default linker** on x86_64-linux-gnu, `cargo publish --workspace`, x86_64-apple-darwin demoted to Tier 2 |
| **1.91** | Oct 2025 | C-style variadic functions, strict arithmetic ops, `core::iter::chain`, `Duration::from_hours`/`from_mins`, `Path::file_prefix`, **aarch64-pc-windows-msvc to Tier 1** |
| **1.92** | Dec 2025 | `&raw` for union fields in safe code, never-type lints deny-by-default, `RwLockWriteGuard::downgrade`, `Box/Rc/Arc::new_zeroed` |
| **1.93** | Jan 2026 | **`asm_cfg`**, `MaybeUninit::assume_init_ref/mut`, `String/Vec::into_raw_parts`, `fmt::from_fn`, `VecDeque::pop_front_if`/`pop_back_if` |

### Key new lints to adopt

**`dangerous_implicit_autorefs`** (warn in 1.88, deny in 1.89) catches implicit autoref of raw pointer dereferences. **`mismatched_lifetime_syntaxes`** (warn in 1.89) detects inconsistent lifetime syntax between parameters and return types. **`function_casts_as_integer`** and **`const_item_interior_mutations`** (both warn in 1.93) catch subtle bugs. The never-type lints **`never_type_fallback_flowing_into_unsafe`** and **`dependency_on_unit_never_type_fallback`** became deny-by-default in 1.92, paving the way for full `!` type stabilization.

---

## Ownership, borrowing, and lifetimes in the modern era

### Lifetime capture rules and precise capturing

The 2024 edition's RPIT capture change is the most consequential ownership-related update. Migration from 2021 may require adding `use<>` bounds where the new default captures too broadly. The **`mismatched_lifetime_syntaxes` lint** (1.89) enforces consistency — mixing named lifetimes (`&'static u8`) with elided lifetimes (`&u8`) in the same function signature now warns:

```rust
// WARNS: inconsistent lifetime syntax
pub fn bad(v: &'static u8) -> &u8 { v }
// FIX:
pub fn good(v: &'static u8) -> &'static u8 { v }
```

### Interior mutability: the complete modern toolkit

| Type | Thread-safe | Lazy init | Stabilized | Replaces |
|------|:-----------:|:---------:|:----------:|----------|
| `Cell<T>` | No | No | 1.0 | — |
| `RefCell<T>` | No | No | 1.0 | — |
| `OnceCell<T>` | No | Yes (set-once) | **1.70** | `once_cell::unsync::OnceCell` |
| `LazyCell<T>` | No | Yes | **1.80** | `once_cell::unsync::Lazy` |
| `OnceLock<T>` | Yes | Yes (set-once) | **1.70** | `once_cell::sync::OnceCell` |
| `LazyLock<T>` | Yes | Yes | **1.80** | `lazy_static!`, `once_cell::sync::Lazy` |
| `Mutex<T>` | Yes | No | 1.0 | — |
| `RwLock<T>` | Yes | No | 1.0 | — |

**`LazyLock` replaces `lazy_static!`** entirely — use it for all global lazy initialization:

```rust
use std::sync::LazyLock;
use std::collections::HashMap;

static CONFIG: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    load_config_from_file()
});
```

**`Cell::update`** (1.88) enables atomic read-modify-write on `Cell` values without separate `get`/`set`. **`RwLockWriteGuard::downgrade`** (1.92) allows downgrading a write lock to a read lock without releasing it. **`DerefMut` for `LazyCell`/`LazyLock`** landed in 1.89.

### Smart pointer selection

| Type | Use when | Thread-safe | Overhead |
|------|----------|:-----------:|----------|
| `Box<T>` | Single owner on heap, recursive types, trait objects | Send if T: Send | 1 pointer |
| `Rc<T>` | Shared ownership, single thread | No | Ref count |
| `Arc<T>` | Shared ownership, multi-thread | Yes | Atomic ref count |
| `Cow<'_, T>` | Zero-copy with owned fallback | Depends on T | enum tag |

**New in 1.92**: `Box::new_zeroed`, `Rc::new_zeroed`, `Arc::new_zeroed` for efficient zero-initialization of heap-allocated memory. **New in 1.91**: `impl Default for Pin<Box<T>>`, `Pin<Rc<T>>`, `Pin<Arc<T>>`.

---

## Async and concurrency: the full modern stack

### Async closures unlock lending futures (1.85)

Async closures solve a fundamental limitation: `|| async {}` closures couldn't return futures that borrow from captures. The three new traits — **`AsyncFn`**, **`AsyncFnMut`**, **`AsyncFnOnce`** — were added to the prelude in all editions:

```rust
// Async closure that borrows captures across await points
let data = vec![1, 2, 3];
let closure = async || {
    process(&data).await  // borrows data — impossible with || async {}
};

// Using in bounds
async fn run<F: AsyncFnOnce() -> i32>(f: F) -> i32 { f().await }
```

All callable types that return futures automatically implement `AsyncFn*`, making this backwards-compatible with existing code.

### Async fn in traits and the Send bound problem

Async fn in traits has been stable since 1.75, but the **Send bound problem** remains the primary friction point. When spawning tasks with a generic async trait, the compiler cannot prove the returned future is `Send`:

```rust
trait Service {
    async fn call(&self) -> Response;
}

// This fails — can't prove S::call()'s future is Send:
fn spawn<S: Service + Send + 'static>(s: S) {
    tokio::spawn(async move { s.call().await });
}
```

**Current best workaround**: Use `#[trait_variant::make(SendService: Send)]` from the `trait-variant` crate (maintained by the Rust async working group). **Return Type Notation** (RFC 3654) will eventually allow `S::call(..): Send` bounds but remains nightly-only, blocked on trait solver work. The **`async-trait`** proc macro is legacy — prefer native async fn in traits for all new code.

### Tokio patterns and structured concurrency

**Tokio** (v1.49, LTS through 2026) remains the dominant async runtime — `async-std` was discontinued in March 2025 (use `smol` as its lightweight successor). Key modern patterns:

**JoinSet for structured task management:**
```rust
use tokio::task::JoinSet;

let mut set = JoinSet::new();
for url in urls {
    set.spawn(async move { fetch(&url).await });
}
while let Some(result) = set.join_next().await {
    handle(result?);
}
// Drop aborts all remaining tasks — structured concurrency
```

**Cancellation with `CancellationToken`** (from `tokio-util`):
```rust
use tokio_util::sync::CancellationToken;

let token = CancellationToken::new();
let child = token.child_token();
tokio::spawn(async move {
    tokio::select! {
        _ = child.cancelled() => { /* graceful shutdown */ }
        _ = long_running_task() => { /* completed */ }
    }
});
token.cancel();  // triggers shutdown
```

**Rule of thumb for mutexes**: Use `std::sync::Mutex` for synchronous critical sections (faster); use `tokio::sync::Mutex` only when the lock must be held across `.await` points.

### Channel ecosystem recommendations

For **async Tokio apps**, use the built-in `tokio::sync::{mpsc, oneshot, broadcast, watch}`. For **sync MPMC**, use `crossbeam-channel`. For **mixed sync/async**, `flume` works (maintenance mode but stable) or `kanal` (fast, but async API is not cancellation-safe). The standard library `std::sync::mpsc` was rewritten using crossbeam internals but lacks MPMC and async support — prefer alternatives for new code.

### What's coming: generators and async iteration

**`gen` blocks** (`#![feature(gen_blocks)]`) produce lazy `Iterator` values via `yield`. The `gen` keyword is reserved in 2024 edition. An experimental `iter!` macro has landed on nightly. Stabilization is not imminent — open design questions around self-referential generators and trait implementation remain. **Stable alternative**: manual `Iterator` implementations or the `genawaiter` crate.

**`AsyncIterator`** exists in std nightly but isn't stabilized. Use **`futures::Stream`** or **`tokio-stream`** for production async iteration:

```rust
use tokio_stream::{self as stream, StreamExt};
let mut s = stream::iter(vec![1, 2, 3]);
while let Some(v) = s.next().await { process(v); }
```

---

## Type system, generics, and const evaluation

### Trait object upcasting eliminates boilerplate (1.86)

`dyn SubTrait` now implicitly coerces to `dyn SuperTrait`:

```rust
trait Base { fn base(&self); }
trait Derived: Base { fn derived(&self); }
fn upcast(x: &dyn Derived) -> &dyn Base { x }  // Just works now
```

### GATs enable lending iterators and zero-copy patterns

Generic Associated Types (stable since 1.65) unlock patterns previously impossible:

```rust
trait LendingIterator {
    type Item<'a> where Self: 'a;
    fn next<'a>(&'a mut self) -> Option<Self::Item<'a>>;
}

trait Cursor {
    type Row<'a>: Debug where Self: 'a;
    fn next_row<'a>(&'a mut self) -> Option<Self::Row<'a>>;
}
```

**Primary associated types in bounds** (1.79) simplify trait usage: `fn process(iter: impl Iterator<Item: Display + Clone>)`.

### Const generics and const evaluation

**Stable const generic capabilities**: integer types (`usize`, `u8`–`u128`, `i8`–`i128`), `bool`, `char` as const parameters. **Generic arg infer** (1.89) allows `[0u8; _]` to infer array lengths. **Inline `const {}` blocks** (1.79) evaluate at compile time within generic contexts:

```rust
fn generic<T>() {
    const { assert!(std::mem::size_of::<T>() <= 64) };  // compile-time check
}
```

**`const fn` capabilities have expanded dramatically**: mutable references (1.83), floating-point arithmetic (1.82), references to statics (1.83), interior mutability via `UnsafeCell` (1.83), and pointer copying (1.93). Calling trait methods in `const fn` requires the unstable `const Trait` feature.

### Diagnostic attributes for library authors

**`#[diagnostic::on_unimplemented]`** (1.78) and **`#[diagnostic::do_not_recommend]`** (1.85) enable dramatically better compile errors:

```rust
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a valid handler",
    note = "Handlers must implement the Handler trait"
)]
trait Handler { /* ... */ }

#[diagnostic::do_not_recommend]
impl<T: Handler, U: Handler> Handler for (T, U) { /* ... */ }
```

### What's approaching stabilization

The **next-gen trait solver** is in production for coherence checking (1.84) and progressing toward full deployment — it unblocks coinductive semantics, implied bounds, and negative impls. **Polonius** (next-gen borrow checker) has a working alpha algorithm with ~5% worst-case overhead and targets 2026 stabilization, unlocking lending iterators and more flexible borrowing patterns. **TAIT** (type-alias impl trait) is blocked on trait solver alignment. The **never type `!`** has had its lints tightened through 1.92 but remains unstable for general type position — use `Infallible` as the stable substitute.

---

## Error handling patterns for 2026

### Library errors: thiserror 2.x

thiserror 2.0 (November 2024) added **`no_std` support**, improved generics handling, and introduced `r#source` for opting out of automatic `Error::source()`. Breaking change: use `{type}` instead of `{r#type}` in format strings.

```rust
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("query failed: {query}")]
    Query { query: String, #[source] cause: SqlError },
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
```

### Application errors: anyhow or eyre

**`anyhow`** (1.x) remains the standard for application error handling. **`color-eyre`** (0.6.x) was **archived in August 2025** — for new projects, use plain `eyre` with custom handlers or simply `anyhow`:

```rust
use anyhow::{Context, Result, ensure};

fn read_config(path: &str) -> Result<Config> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {path}"))?;
    let config: Config = toml::from_str(&content).context("parse error")?;
    ensure!(config.version > 0, "version must be positive");
    Ok(config)
}
```

**Key principle**: Libraries expose typed errors via `thiserror`; applications consume them via `anyhow`/`eyre`. Never re-export `anyhow::Error` from a library's public API.

The `std::error::Error` trait's `backtrace()` method and `provide()` for generic member access remain **unstable**. For backtrace support on stable, rely on `anyhow` (captures automatically) or use the `backtrace` crate directly.

---

## Cargo, build system, and project structure

### Cargo 2024 edition defaults

A 2024 edition `Cargo.toml` implies **resolver v3** (MSRV-aware dependency resolution), rejects `[project]` (must use `[package]`), and requires hyphenated field names (`dev-dependencies` not `dev_dependencies`):

```toml
[package]
name = "my-app"
version = "0.1.0"
edition = "2024"
rust-version = "1.85.0"
```

**Resolver v3** considers `package.rust-version` when selecting dependency versions, preferring MSRV-compatible versions with fallback to newer ones.

### Workspace management with inheritance

Modern workspaces centralize metadata, dependencies, and lints:

```toml
# Root Cargo.toml
[workspace]
members = ["crates/*"]

[workspace.package]
edition = "2024"
rust-version = "1.85.0"
license = "MIT OR Apache-2.0"

[workspace.dependencies]
tokio = { version = "1.40", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
thiserror = "2.0"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
unwrap_used = "warn"
```

Member crates inherit with `version.workspace = true`, `tokio.workspace = true`, and `[lints] workspace = true`.

### Build performance configuration

**`lld` is the default linker on `x86_64-unknown-linux-gnu`** since 1.90, delivering up to **7× faster linking**. For other platforms or maximum speed, configure `mold`:

```toml
# .cargo/config.toml
[target.'cfg(target_os = "linux")']
rustflags = ["-C", "link-arg=-fuse-ld=mold"]

[build]
rustc-wrapper = "sccache"  # compile caching
```

**Recommended dev profile** for fast iteration:

```toml
[profile.dev]
debug = "line-tables-only"  # faster builds, useful backtraces

[profile.dev.package."*"]
debug = false  # no debug info for dependencies
```

**Release profile for maximum performance:**

```toml
[profile.release]
lto = "thin"
codegen-units = 1
strip = "symbols"
panic = "abort"
```

### Recent Cargo stabilizations

**`cargo publish --workspace`** (1.90) publishes all workspace crates in dependency order. **`build.build-dir`** (1.91) allows configurable intermediate artifact directories. **Config `include` key** (1.93) allows loading additional config files. **Cargo cache garbage collection** is now automatic (1.88).

### Cargo script (approaching stabilization)

Single-file Rust programs with embedded dependencies use frontmatter syntax. Still nightly-only but a 2025H2 project goal with active stabilization work:

```rust
#!/usr/bin/env cargo
---
[dependencies]
clap = { version = "4", features = ["derive"] }
---

use clap::Parser;
#[derive(Parser)]
struct Args { name: String }
fn main() { println!("Hello, {}!", Args::parse().name); }
```

### Supply chain security

Run these tools in CI: **`cargo-audit`** scans against RustSec advisories, **`cargo-deny`** checks licenses and bans, **`cargo-vet`** verifies human audits exist (imports Mozilla/Google audit data). **`cargo-auditable`** embeds dependency metadata in binaries for post-build scanning — used by Alpine Linux, NixOS, and Microsoft.

---

## Testing: the modern toolkit

### Built-in framework improvements

**Combined doctests** (2024 edition) compile all doctests into a single binary instead of individually — the Jiff crate saw compilation drop from **12.56s to 0.25s**. Use the `standalone_crate` tag for tests needing isolation.

### cargo-nextest as the standard test runner

`cargo-nextest` runs each test in its own process (better isolation), parallelizes across binaries, detects flaky tests with automatic retries, and provides clean CI output. It does not support doctests — run `cargo test --doc` separately.

```toml
# .config/nextest.toml
[profile.default]
retries = 2
slow-timeout = { period = "60s", terminate-after = 2 }

[profile.ci]
retries = 3
junit = { path = "target/nextest/ci/junit.xml" }
```

### Testing ecosystem recommendations

- **Snapshot testing**: `insta` — review with `cargo insta review`, redact dynamic fields
- **Property-based testing**: `proptest` — automatic shrinking to minimal failing case
- **Mocking**: `mockall` 0.13 — `#[automock]` on traits
- **Fuzz testing**: `cargo-fuzz` with libfuzzer
- **Coverage**: `cargo-llvm-cov` for LLVM-based source coverage
- **Benchmarking**: **`divan`** for simplicity (attribute-based, allocation profiling), **`criterion`** for statistical rigor and HTML reports. Both require `harness = false` in Cargo.toml

---

## API design and code quality

### Builder pattern: bon leads the modern approach

**`bon`** (v3.8) provides compile-time checked builders for both structs and functions, used by crates.io, tantivy, and apache-avro:

```rust
use bon::Builder;

#[derive(Builder)]
struct Server {
    host: String,
    port: u16,
    max_connections: Option<u32>,
}

let server = Server::builder()
    .host("localhost".to_owned())
    .port(8080)
    .build();
```

`bon` also works on functions via `#[builder]`. Prefer `bon` over `typed-builder` (fewer features) or manual builders (runtime errors).

### Clippy and rustfmt for 2024 edition

Configure lints in `Cargo.toml` instead of scattered attributes:

```toml
[lints.clippy]
pedantic = { level = "warn", priority = -1 }
unwrap_used = "deny"
missing_errors_doc = "warn"
missing_panics_doc = "warn"
```

**`rustfmt` style edition 2024** brings version-sorting (NonZeroU8 before NonZeroU16) and improved formatting. Set independently of Rust edition:

```toml
# rustfmt.toml
style_edition = "2024"
```

### Visibility and newtype patterns

Use `pub(crate)` liberally for internal APIs. The **newtype pattern** provides type safety through the compiler:

```rust
pub struct UserId(u64);
pub struct Email(String);

impl Email {
    pub fn new(s: impl Into<String>) -> Result<Self, ValidationError> {
        let s = s.into();
        if s.contains('@') { Ok(Email(s)) } else { Err(ValidationError) }
    }
    pub fn as_str(&self) -> &str { &self.0 }
}
```

**Never implement `Deref`/`DerefMut` on newtypes** — only smart pointers should use these traits. Provide explicit `as_str()`, `as_inner()`, or `AsRef` implementations instead.

---

## Serialization with serde and alternatives

### serde best practices

serde (1.0.228) remains ubiquitous. Key patterns for the 2024 edition:

```rust
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
    #[serde(default)]
    app_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    optional_field: Option<String>,
    #[serde(borrow)]
    description: &'a str,  // zero-copy deserialization
}
```

Four enum representation strategies: **externally tagged** (default), **internally tagged** (`#[serde(tag = "type")]`), **adjacently tagged** (`#[serde(tag = "t", content = "c")]`), **untagged** (`#[serde(untagged)]`). Use the `serde_with` crate for common custom serialization (Duration as seconds, hex encoding, etc.).

### Binary format selection

| Crate | Best for | Key trait |
|-------|----------|-----------|
| **bitcode** 0.6 | General purpose — best speed AND compression | serde-based |
| **rkyv** 0.8 | Read-heavy workloads — zero-copy access | Own derive system |
| **postcard** | Embedded / no_std, compact wire format | serde-based |
| **bincode** 2.0 | **Discontinued** — use bitcode or postcard | — |

**bitcode** is the overall benchmark winner for both serialization speed and compression ratio. **rkyv** provides near-instant "deserialization" via zero-copy memory access — ideal for mmap'd files and caches but lacks schema evolution.

---

## Web, networking, and the modern stack

### axum + tokio + tower: the standard stack

**axum 0.8** (January 2025) is the dominant web framework. Key 0.8 change: path parameters use `/{id}` syntax (OpenAPI-style) instead of `/:id`:

```rust
use axum::{routing::{get, post}, extract::{Path, Json, State}, Router};
use std::sync::Arc;

async fn get_user(Path(id): Path<u32>) -> Json<User> { /* ... */ }
async fn create_user(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateUser>,
) -> (StatusCode, Json<User>) { /* ... */ }

let app = Router::new()
    .route("/users/{id}", get(get_user))
    .route("/users", post(create_user))
    .layer(TraceLayer::new_for_http())
    .with_state(Arc::new(state));

axum::serve(TcpListener::bind("0.0.0.0:3000").await?, app).await?;
```

**tower** (0.5) is exploring migration to `async fn call(&self, req)` (dropping `poll_ready`), which would dramatically simplify middleware. The `tower-async` fork already implements this. **tower-http** 0.6 provides `TraceLayer`, `CorsLayer`, `CompressionLayer`, and more.

**reqwest** (0.13) switched its default TLS to **rustls** — prefer rustls over OpenSSL bindings for all new projects. HTTP/3 support is experimental via the `http3` feature using `quinn`.

### Database recommendations

**sqlx 0.8** for async, compile-time-checked SQL queries (requires a live database at compile time). **diesel 2.3** for maximum compile-time type safety in sync contexts (security audit completed October 2025). **sea-orm 1.x** for rapid CRUD development with ORM abstractions built on sqlx.

---

## CLI, observability, and data structures

### CLI with clap 4.x derive

```rust
#[derive(clap::Parser)]
#[command(version, about)]
struct Cli {
    #[arg(short, long)]
    config: Option<PathBuf>,
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,
    #[command(subcommand)]
    command: Commands,
}
```

For terminal UIs, **ratatui** (successor to tui-rs) with **crossterm** is the standard. Use **owo-colors** for terminal colors and **indicatif** for progress bars.

### Structured observability with tracing

The **`tracing`** framework (0.1) is the standard for structured, async-aware diagnostics — prefer it over `log` + `env_logger`:

```rust
use tracing::{info, instrument};

#[instrument(skip(db), fields(user_id = %id))]
async fn get_user(id: u64, db: &Pool) -> Result<User, Error> {
    info!("Fetching user");
    Ok(db.query(id).await?)
}

// Setup with composable layers
tracing_subscriber::registry()
    .with(EnvFilter::new("info,tower_http=debug"))
    .with(fmt::layer().json())
    .init();
```

For **rich error diagnostics** in CLI tools, use **miette** (v7.6) which provides compiler-quality error output with source code snippets and labels.

### Hash map internals updated

The standard library `HashMap` uses **hashbrown** internally, which now defaults to **foldhash** (replacing ahash) as of hashbrown 0.15 — faster hashing, better distribution, and smaller `HashMap` size (40 bytes vs 64). For most code, just use `std::collections::HashMap`.

For concurrent maps: **dashmap** for write-heavy workloads, **papaya** for read-heavy async-safe workloads with predictable latency (lock-free). **slotmap** provides generational arenas for stable entity handles (ideal for ECS patterns). **indexmap** preserves insertion order.

---

## Unsafe Rust, FFI, and safety tooling

### Strict provenance APIs (1.84)

Modern pointer manipulation respects provenance:

```rust
let ptr = &x as *const i32;
let addr: usize = ptr.addr();           // address without losing provenance
let new_ptr = ptr.with_addr(addr);      // reconstruct with provenance
let tagged = ptr.map_addr(|a| a | 0x1); // tag while preserving provenance
```

### FFI with cxx

**`cxx`** (1.0) provides **safe** C++ interop by owning both sides of the FFI boundary with compile-time signature verification. Prefer over raw `bindgen` for C++ interop:

```rust
#[cxx::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("mylib.h");
        type MyClass;
        fn process(&self, input: &CxxString) -> i32;
    }
    extern "Rust" {
        fn rust_callback(value: i32) -> String;
    }
}
```

### Miri for detecting undefined behavior

Miri's capabilities expanded significantly in 2025: **native C FFI support**, non-deterministic floating-point simulation, and `-Zmiri-many-seeds` for exploring multiple executions. Run in CI on all unsafe code paths:

```yaml
- run: |
    rustup toolchain install nightly --component miri
    cargo +nightly miri test
```

---

## Embedded, WebAssembly, and security

### Embassy for async embedded

**Embassy** is the leading async embedded framework, supporting STM32, nRF, RP2040/RP235x, and ESP32 families. Combined with **embedded-hal 1.0** traits and **defmt** for efficient logging:

```rust
#![no_std]
#![no_main]
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};

#[embassy_executor::main]
async fn main(_spawner: Spawner) {
    let p = embassy_stm32::init(Default::default());
    let mut led = Output::new(p.PB7, Level::Low, Speed::Low);
    loop {
        led.toggle();
        Timer::after(Duration::from_millis(500)).await;
    }
}
```

### WebAssembly targets updated

**`wasm32-wasi` was removed in 1.91** — use `wasm32-wasip1` or `wasm32-wasip2` (Tier 2, supports Component Model). **WASI 0.3** targeting February 2026 adds native async support. For frontend, **Leptos** leads with fine-grained reactivity (like SolidJS), streaming SSR, and the smallest Wasm binaries. **Dioxus** excels for cross-platform (web + desktop + mobile via WebView).

### Cryptography: rustls as the default

**rustls** (0.23) is the recommended TLS library with pluggable crypto providers: `aws-lc-rs` (default, FIPS support), `ring`, or RustCrypto crates. Use **`secrecy`** for zeroize-on-drop secret handling and **`subtle`** for constant-time comparisons.

---

## Performance optimization techniques

### Recommended release optimization pipeline

For maximum runtime performance, apply in order: **LTO** (`lto = "thin"` or `"fat"`), **single codegen unit** (`codegen-units = 1`), **PGO** (profile-guided optimization — typically **10%+ improvement**), and optionally **BOLT** for post-link optimization. The `cargo-pgo` tool simplifies the PGO workflow:

```bash
cargo pgo instrument build && cargo pgo instrument run -- <workload>
cargo pgo optimize build
```

**Safe `#[target_feature]`** (1.86) allows marking safe functions with SIMD requirements — they can be called safely from functions with matching features. Most `std::arch` SIMD intrinsics became callable from safe code in 1.87 when the target feature is enabled. **Portable SIMD (`std::simd`) remains nightly-only** with no near-term stabilization expected — use the `wide` crate for stable portable SIMD.

---

## Recommended ecosystem crate versions (early 2026)

| Category | Crate | Version | Notes |
|----------|-------|---------|-------|
| Async runtime | `tokio` | 1.49 | Multi-thread default; LTS through 2026 |
| Web framework | `axum` | 0.8 | `/{param}` syntax; tower-native |
| HTTP client | `reqwest` | 0.13 | rustls default |
| Serialization | `serde` | 1.0.228 | Ubiquitous |
| CLI | `clap` | 4.5 | Derive API |
| Error (lib) | `thiserror` | 2.0 | no_std support |
| Error (app) | `anyhow` | 1.x | Or `eyre` 0.6 |
| Diagnostics | `miette` | 7.6 | Rich source-aware errors |
| Logging | `tracing` | 0.1 | Structured, async-aware |
| Database | `sqlx` | 0.8 | Compile-time checked |
| Test runner | `cargo-nextest` | latest | Process-per-test |
| Snapshots | `insta` | latest | `cargo insta review` |
| Benchmarks | `divan` | 0.1 | Attribute-based, allocation profiling |
| Builder | `bon` | 3.8 | Struct + function builders |
| Binary format | `bitcode` | 0.6 | Fastest + smallest |
| TLS | `rustls` | 0.23 | Prefer over OpenSSL |
| Concurrent map | `papaya` | latest | Lock-free, async-safe |
| Embedded | `embassy` | git | Async embedded framework |
| Frontend (Wasm) | `leptos` | latest | Fine-grained reactivity |

---

## Project goals and the road ahead

The Rust project's **2025H2 goals** (41 goals, 13 flagships) and **draft 2026 goals** (45 goals) reveal clear priorities for the language's evolution.

**Polonius stabilization** targets 2026, finally resolving the long-standing limitations of the current borrow checker and enabling patterns like lending iterators. The **next-gen trait solver** is in production for coherence and approaching general stabilization — it unblocks coinductive semantics, implied bounds, and a cleaner type system foundation. **Pin ergonomics** (`&pin mut T` syntax) is under active experimentation on nightly, aiming to make self-referential types dramatically easier to work with.

The **"Beyond the `&`"** initiative (reborrow traits, field projections, ergonomic ref-counting) aims to make user-defined smart pointers as ergonomic as references — RFC #3873 (reborrow traits) is in final comment period. **`cargo script`** stabilization is imminent, with frontmatter syntax support merged and tooling integration progressing. **`build-std`** MVP is targeting stabilization for embedded and Rust-for-Linux use cases.

The **2026 flagship themes** are: "Just Add Async" (sync patterns should work in async), "Beyond the `&`" (smart pointer ergonomics), "Unblocking Dormant Traits" (lending iterators, extern types), "Constify All The Things" (const generics expansion, compile-time introspection), "Higher-Level Rust" (scripts with dependencies), and "Secure Your Supply Chain" (breaking change detection, SBOM generation). Three new **Wasm Component Model** compiler targets and sanitizer stabilization are also on the roadmap.

## Conclusion

Rust in early 2026 is a language in confident stride. The 2024 edition's implicit lifetime captures and stricter unsafe semantics make code simultaneously safer and more ergonomic. The nine releases from 1.85 to 1.93 form a remarkably productive period — trait object upcasting, `let` chains, async closures, and `lld`-by-default each independently improve the developer experience. The ecosystem has consolidated around clear winners: axum for web, tokio for async, tracing for observability, sqlx for databases, and bon for builders. Binary serialization quietly shifted with bitcode emerging as the performance leader and bincode's discontinuation.

The most architecturally significant near-term changes are Polonius (unlocking lending patterns the borrow checker currently rejects), the next-gen trait solver (fixing type system soundness holes and enabling new abstractions), and pin ergonomics (making async combinators dramatically simpler). Teams starting greenfield projects today should structure code to take advantage of these features the moment they stabilize — use trait-based abstractions that Polonius will make more flexible, prefer `async fn` in traits over workarounds that will become unnecessary, and watch for `gen` blocks to replace manual iterator implementations. The path from here is clear: Rust is becoming both more powerful and more approachable with every six-week release.
