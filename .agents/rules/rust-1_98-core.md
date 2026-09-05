---
type: "agent_requested"
description: "Rust 1.98 coding guidelines"
---
# Rust 1.98 Production Guidelines: Idiomatic Async Services and Systems Code

Rust 1.98 (stable, Rust 2024 edition) is a mature systems language whose defining strength is compile-time guarantees: the borrow checker, `Send`/`Sync`, exhaustive `match`, and a zero-cost async model let you encode invariants that other languages defer to runtime. Optimize for making illegal states unrepresentable, for pushing fallibility into the type system (`Result`, `Option`, typed errors), and for letting ownership—not defensive copying or runtime locks—drive your design. The 2024 edition is the current default and unlocks let-chains, the new RPIT lifetime-capture rules, and `unsafe extern` blocks; write all new crates against it.

The most common way agents write wrong-but-plausible Rust is by importing habits from garbage-collected or exception-based ecosystems: reaching for `.clone()`, `Arc<Mutex<_>>`, or `.unwrap()` to silence the borrow checker instead of restructuring ownership; using `async` for CPU-bound work; blocking inside async tasks; catching errors as opaque strings instead of typed enums; and pulling in an incumbent crate (native-tls, `#[async_trait]`, `error-chain`) where the current ecosystem has moved on. This reference teaches the modern default for each role and the specific trap in the alternative.

## Toolchain, edition, and project layout

Pin the toolchain per repository with `rust-toolchain.toml` so every developer and CI runner resolves the same compiler; this file also governs `rustfmt` and `clippy`.

```toml
# rust-toolchain.toml
[toolchain]
channel = "1.98.0"
components = ["rustfmt", "clippy", "rust-src"]
profile = "minimal"
```

Set the edition and MSRV explicitly in every manifest. `edition` selects the language mode; `rust-version` is the deployment floor Cargo enforces during resolution (the v3 resolver honors `rust-version` and will not pick a dependency that requires a newer compiler).

```toml
# Cargo.toml
[package]
name = "taskr"
version = "0.1.0"
edition = "2024"
rust-version = "1.98"

[dependencies]
tokio = { version = "1.53", features = ["rt-multi-thread", "macros", "signal", "net", "time", "sync", "fs"] }
axum = "0.8"
axum-extra = { version = "0.12", features = ["typed-header"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["trace", "timeout", "compression-br", "cors"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sqlx = { version = "0.9", features = ["runtime-tokio", "tls-rustls-ring", "postgres", "macros", "uuid", "chrono", "migrate"] }
thiserror = "2"
anyhow = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
clap = { version = "4", features = ["derive", "env"] }
reqwest = { version = "0.13", features = ["json"] }
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }

[dev-dependencies]
insta = { version = "1", features = ["json"] }
rstest = "0.23"
tokio = { version = "1.53", features = ["test-util", "macros", "rt-multi-thread"] }

[lints.clippy]
# Project-wide lint policy lives in the manifest, not in lib.rs attributes.
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
unwrap_used = "warn"
expect_used = "warn"
```

Everyday commands, all reading the pinned toolchain:

```bash
cargo fmt --all                       # format (rustfmt, style edition follows the crate edition)
cargo clippy --all-targets --all-features -- -D warnings   # lint, deny on CI
cargo nextest run                     # fast, isolated test runner (see Testing)
cargo test --doc                      # doctests (nextest does not run these)
cargo build --release
```

Prefer a Cargo **workspace** once you have more than one binary or a shared library; put common dependency versions in `[workspace.dependencies]` and reference them with `pkg = { workspace = true }` so versions stay unified in one place.

## Ownership, types, and making illegal states unrepresentable

The core discipline: model your domain so the compiler rejects invalid combinations, and borrow rather than clone. Reach for `.clone()` only when you genuinely need a second owned value, not to escape a borrow error—usually the fix is to reorder operations or pass a `&T`.

Use the newtype pattern to give primitive values meaning and to centralize validation. Parse untrusted input into a validated type once, at the boundary, and let the rest of the code trust the type.

```rust
use std::fmt;

/// A validated, non-empty task title of at most 200 chars.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Title(String);

#[derive(Debug, thiserror::Error)]
#[error("title must be 1..=200 characters, got {0}")]
pub struct TitleError(usize);

impl Title {
    pub fn parse(raw: impl Into<String>) -> Result<Self, TitleError> {
        let raw = raw.into();
        let len = raw.chars().count();
        if (1..=200).contains(&len) {
            Ok(Self(raw))
        } else {
            Err(TitleError(len))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Title {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
```

Model states as enums, not as structs full of `Option`s and booleans that permit contradictory combinations:

```rust
use chrono::{DateTime, Utc};

/// A task can only be in exactly one lifecycle state; the type forbids
/// "completed but also has a due date in the future", etc.
#[derive(Debug, Clone)]
pub enum Status {
    Open { due: Option<DateTime<Utc>> },
    InProgress { since: DateTime<Utc> },
    Done { completed_at: DateTime<Utc> },
    Cancelled { reason: String },
}

impl Status {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Status::Done { .. } | Status::Cancelled { .. })
    }
}
```

Prefer iterator chains over index loops; they are bounds-check-friendly and express intent. Use `?` on `Option`/`Result` to propagate absence and failure instead of nesting. When you must handle "not found" distinctly from "error", keep them as separate types (`Option<T>` vs `Result<T, E>`) rather than collapsing to a sentinel.

## Error handling: typed errors in libraries, context in applications

The community standard is `thiserror` (2.x) for library error enums and `anyhow` (1.x) for application code that just needs to propagate and report. Never return `Box<dyn Error>` from a library API when callers may need to branch on the failure, and never stringly-type errors.

Define one error enum per layer, derive `Error`, and use `#[from]` to make `?` compose. `#[error(transparent)]` forwards an underlying error's `Display` and `source` unchanged—use it for a passthrough "other" variant.

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RepoError {
    #[error("task {0} not found")]
    NotFound(uuid::Uuid),

    #[error("title validation failed")]
    Title(#[from] crate::domain::TitleError),

    // Preserves the sqlx error as the `source` of this variant.
    #[error("database error")]
    Database(#[from] sqlx::Error),
}
```

In application/binary code, use `anyhow::Result` and attach `.context(...)` at each boundary so a failure reads as a chain from cause to symptom:

```rust
use anyhow::Context as _;

fn load_config(path: &std::path::Path) -> anyhow::Result<Config> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading config at {}", path.display()))?;
    let cfg: Config = toml::from_str(&text)
        .with_context(|| format!("parsing config at {}", path.display()))?;
    Ok(cfg)
}
```

Rule of thumb: if a *caller* needs to match on the failure, return a typed `thiserror` enum. If the failure only ever gets logged or shown to a human, `anyhow` is correct and lighter. Do not derive `thiserror` on an enum whose variants callers never distinguish—that is boilerplate for nothing.

## Async and concurrency with Tokio

Tokio (1.x) is the de-facto async runtime; axum, sqlx, reqwest, and tonic all build on it. Use `#[tokio::main]` for the entrypoint and `#[tokio::test]` for async tests. Enable only the features you use, but `rt-multi-thread`, `macros`, and the I/O features (`net`, `time`, `sync`) are the common core.

The single most important async rule: **never block the runtime.** An `async fn` must not call blocking file/network syscalls, do heavy CPU work, or hold a `std::sync::Mutex` across an `.await`. Blocking a worker thread starves every other task scheduled on it.

- CPU-bound work → `tokio::task::spawn_blocking` (or a `rayon` pool for data parallelism), then `.await` the handle.
- Synchronous library calls → `spawn_blocking`.
- Shared mutable state held across `.await` → `tokio::sync::Mutex` / `RwLock`, not `std::sync::Mutex`. For state *not* held across await points, a `std::sync::Mutex` in a short non-async critical section is fine and faster.

```rust
use std::time::Duration;
use tokio::time::timeout;

/// Runs a blocking hash on the blocking pool and bounds it with a timeout,
/// so a slow call cannot wedge the caller.
pub async fn hash_password(password: String) -> anyhow::Result<String> {
    let handle = tokio::task::spawn_blocking(move || {
        // Pretend this is bcrypt/argon2 — CPU-bound, must not run on a worker.
        expensive_hash(&password)
    });
    let hashed = timeout(Duration::from_secs(5), handle)
        .await
        .map_err(|_| anyhow::anyhow!("hashing timed out"))?  // timeout elapsed
        .map_err(|e| anyhow::anyhow!("hashing task panicked: {e}"))?; // JoinError
    Ok(hashed)
}

fn expensive_hash(_pw: &str) -> String {
    "…".to_string()
}
```

Spawned tasks must be `Send + 'static`; that is why `Rc`/`RefCell` cannot cross a `tokio::spawn`—use `Arc` and a Tokio sync primitive. For structured concurrency, use `JoinSet` to run a dynamic set of tasks and collect results as they finish, and `tokio_util::sync::CancellationToken` (from `tokio-util`) or a `tokio::sync::watch` channel to propagate shutdown.

```rust
use tokio::task::JoinSet;

/// Fetch many URLs concurrently with a bounded fan-out; failures are per-item,
/// not fatal to the whole batch.
pub async fn fetch_all(client: &reqwest::Client, urls: Vec<String>) -> Vec<(String, anyhow::Result<u16>)> {
    let mut set = JoinSet::new();
    for url in urls {
        let client = client.clone(); // reqwest::Client is Arc inside — cheap to clone.
        set.spawn(async move {
            let status = client.get(&url).send().await.map(|r| r.status().as_u16());
            (url, status.map_err(anyhow::Error::from))
        });
    }
    let mut out = Vec::new();
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(result) => out.push(result),
            Err(join_err) => tracing::error!(%join_err, "fetch task panicked"),
        }
    }
    out
}
```

Two async gotchas agents hit: **cancellation** (any future dropped at an `.await` point simply stops—hold no half-applied state across awaits that must be rolled back; prefer transactional or idempotent operations), and **`select!` branch cancellation** (losing branches are dropped, so do not `select!` over a future that has irrevocable side effects mid-poll). Async closures (`async || { … }`, stable since 1.85) and the `AsyncFn`/`AsyncFnMut`/`AsyncFnOnce` traits let you write higher-order async APIs directly instead of the old "closure returning an async block" workaround; use them for retry/middleware helpers.

Graceful shutdown ties it together: listen for a signal and let in-flight work drain.

```rust
pub async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        signal(SignalKind::terminate()).expect("install SIGTERM handler").recv().await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
```

## HTTP services with axum and tower

axum (0.8) is the default web framework: Tokio-native, built on hyper 1.0 and tower, with no routing macros. Handlers are plain `async fn`s; request data is declared through typed **extractors** as function parameters, and anything implementing `IntoResponse` can be returned. axum 0.8 uses native `async fn` in traits—do **not** add `#[async_trait]` to extractor or handler impls, and note the path parameter syntax is `{id}` (brace style), not the older `:id`.

Share application state with the `State` extractor over a cheaply-cloneable struct (wrap non-`Clone` resources in `Arc`). Put cross-cutting concerns—tracing, timeouts, compression, CORS—in tower/`tower-http` layers rather than repeating them per handler.

```rust
use std::sync::Arc;
use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use tower_http::{timeout::TimeoutLayer, trace::TraceLayer};
use std::time::Duration;
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub db: sqlx::PgPool, // PgPool is Arc inside; clone is cheap.
}

#[derive(Deserialize)]
pub struct CreateTask {
    pub title: String,
}

#[derive(Serialize)]
pub struct TaskDto {
    pub id: Uuid,
    pub title: String,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/tasks", post(create_task))
        .route("/tasks/{id}", get(get_task))
        .layer(TraceLayer::new_for_http())
        .layer(TimeoutLayer::new(Duration::from_secs(15)))
        .with_state(state)
}

async fn create_task(
    State(state): State<AppState>,
    Json(body): Json<CreateTask>,
) -> Result<(StatusCode, Json<TaskDto>), ApiError> {
    let title = crate::domain::Title::parse(body.title)?; // TitleError -> ApiError
    let rec = sqlx::query!(
        "INSERT INTO tasks (id, title) VALUES ($1, $2) RETURNING id, title",
        Uuid::new_v4(),
        title.as_str(),
    )
    .fetch_one(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(TaskDto { id: rec.id, title: rec.title })))
}

async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<TaskDto>, ApiError> {
    let rec = sqlx::query!("SELECT id, title FROM tasks WHERE id = $1", id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(TaskDto { id: rec.id, title: rec.title }))
}
```

Make your error type implement `IntoResponse` once, so handlers can use `?` and every failure maps to a correct status code and JSON body. This is the idiomatic axum error pattern.

```rust
pub enum ApiError {
    NotFound,
    Validation(String),
    Internal(anyhow::Error),
}

impl From<crate::domain::TitleError> for ApiError {
    fn from(e: crate::domain::TitleError) -> Self {
        ApiError::Validation(e.to_string())
    }
}
impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => ApiError::NotFound,
            other => ApiError::Internal(other.into()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found".to_string()),
            ApiError::Validation(m) => (StatusCode::UNPROCESSABLE_ENTITY, m),
            ApiError::Internal(e) => {
                tracing::error!(error = ?e, "internal error"); // log detail, don't leak it
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error".to_string())
            }
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}
```

Serve it with graceful shutdown wired to the signal handler above:

```rust
pub async fn serve(state: AppState) -> anyhow::Result<()> {
    let app = router(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    tracing::info!("listening on {}", listener.local_addr()?);
    axum::serve(listener, app)
        .with_graceful_shutdown(crate::shutdown_signal())
        .await?;
    Ok(())
}
```

## Data layer with SQLx and PostgreSQL

SQLx (0.9) is the default database toolkit: async, pure-Rust drivers, and **compile-time-checked** queries against a live schema—no ORM, no DSL. Prefer the `query!`/`query_as!` macros; they validate SQL and infer result types at build time. Use `sqlx::query` (unchecked) only for dynamic SQL you build at runtime. Note SQLx 0.9 requires runtime SQL to be a `SqlSafeStr` (string literals qualify), which blocks accidental injection of arbitrary runtime strings.

Set `DATABASE_URL` (a `.env` file works via `sqlx-cli`) so the macros can reach the database at compile time, or commit an offline query cache (`cargo sqlx prepare`) so CI builds without a database.

```rust
use sqlx::postgres::PgPoolOptions;

pub async fn connect(url: &str) -> anyhow::Result<sqlx::PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(16)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(url)
        .await?;
    // Migrations are embedded at compile time from ./migrations.
    sqlx::migrate!().run(&pool).await?;
    Ok(pool)
}
```

Map rows into domain structs with `query_as!`. `FromRow` derive is for the non-macro API; the macros build the struct from column names directly.

```rust
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Debug)]
pub struct TaskRow {
    pub id: Uuid,
    pub title: String,
    pub created_at: DateTime<Utc>,
}

pub async fn recent_tasks(pool: &sqlx::PgPool, limit: i64) -> Result<Vec<TaskRow>, sqlx::Error> {
    sqlx::query_as!(
        TaskRow,
        "SELECT id, title, created_at FROM tasks ORDER BY created_at DESC LIMIT $1",
        limit,
    )
    .fetch_all(pool)
    .await
}
```

For multi-statement invariants, use an explicit transaction; it rolls back automatically if dropped without `commit`, which composes correctly with `?` and with task cancellation.

```rust
pub async fn transfer_owner(
    pool: &sqlx::PgPool,
    task: Uuid,
    new_owner: Uuid,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    sqlx::query!("UPDATE tasks SET owner = $1 WHERE id = $2", new_owner, task)
        .execute(&mut *tx) // note: &mut *tx, deref the Transaction to its connection
        .await?;
    sqlx::query!("INSERT INTO audit (task, owner) VALUES ($1, $2)", task, new_owner)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?; // omit commit -> Drop rolls back
    Ok(())
}
```

For bulk inserts, pass Postgres arrays with `= ANY($1)` / `UNNEST` rather than building an `IN (?, ?, …)` list—one prepared statement instead of a combinatorial explosion of query variants.

## Serialization with serde

serde (1.x) with `serde_json` is the universal serialization layer; derive `Serialize`/`Deserialize` on DTOs. Use attributes to shape the wire format rather than writing manual impls: `#[serde(rename_all = "camelCase")]` for JSON APIs, `#[serde(default)]` for optional fields, `#[serde(skip_serializing_if = "Option::is_none")]` to omit nulls, and `#[serde(deny_unknown_fields)]` on inputs you want to validate strictly.

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateTaskRequest {
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due: Option<chrono::DateTime<chrono::Utc>>,
}
```

Deserialize into a permissive DTO at the boundary, then convert into your validated domain type—do not scatter `deny_unknown_fields` and validation across business logic. For enums, `#[serde(tag = "type")]` produces internally-tagged JSON that maps cleanly to a discriminated union on the client.

## Observability with tracing

Use `tracing` (0.1) for structured, async-aware diagnostics—not `log`, and never `println!` for diagnostics in a service. Initialize a `tracing-subscriber` once at startup with an `EnvFilter` (honoring `RUST_LOG`) and JSON output in production. Instrument functions with `#[tracing::instrument]` to get a span per call with arguments as fields; use `skip`/`skip_all` to avoid logging large or sensitive arguments.

```rust
use tracing_subscriber::{layer::SubscriberExt as _, util::SubscriberInitExt as _, EnvFilter};

pub fn init_telemetry() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().json())
        .init();
}

#[tracing::instrument(skip(pool), fields(task_count))]
pub async fn purge_done(pool: &sqlx::PgPool) -> Result<u64, sqlx::Error> {
    let result = sqlx::query!("DELETE FROM tasks WHERE status = 'done'")
        .execute(pool)
        .await?;
    let n = result.rows_affected();
    tracing::Span::current().record("task_count", n);
    tracing::info!(deleted = n, "purged completed tasks");
    Ok(n)
}
```

Emit events with structured fields (`tracing::info!(user_id = %id, "…")`), not interpolated strings—fields are queryable in a log backend. Use `tracing-subscriber` ≥ 0.3.20: earlier versions carried RUSTSEC-2025-0055 / CVE-2025-58160 (reported 2025-08-29), an ANSI escape-sequence injection where untrusted input logged to a terminal could manipulate the display; the fix (escaping ANSI control characters) shipped in 0.3.20, so pin at or above it (the current release is 0.3.23).

## HTTP clients and outbound calls

reqwest (0.13) is the default async HTTP client. As of 0.13 it defaults to **rustls** (with the aws-lc-rs crypto provider) rather than native-tls—prefer the default; only enable `native-tls` when you must use the OS trust store or a corporate MITM proxy. `query` and `form` are now opt-in crate features. Build one `Client` and reuse it: it holds a connection pool internally and is cheap to clone.

```rust
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .connect_timeout(std::time::Duration::from_secs(3))
        .user_agent(concat!("taskr/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("client config is valid")
}
```

Crypto-provider note: the sqlx config above uses the rustls **ring** backend while reqwest 0.13 defaults to **aws-lc-rs**. Both work, but if any code calls rustls APIs directly and no process-wide default is installed, you will hit a "no process-level `CryptoProvider`" panic. Standardize on one backend across the workspace—switch sqlx to its aws-lc-rs rustls feature to match reqwest—if you touch rustls directly.

For CLI tools that make a single request, be aware reqwest pulls in the full Tokio + TLS stack; a small blocking client (`ureq`) can be the better fit there. Within an async service, reqwest is the right default.

## Command-line interfaces with clap

clap (4.x) with the `derive` feature is the standard argument parser. Define the CLI as a struct; derive `Parser`. Use subcommands as an enum, `#[arg(env)]` to fall back to environment variables, and `value_enum` for closed choice sets.

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "taskr", version, about = "task service and admin CLI")]
pub struct Cli {
    #[arg(long, env = "DATABASE_URL")]
    pub database_url: String,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run the HTTP server.
    Serve {
        #[arg(long, default_value_t = 8080)]
        port: u16,
    },
    /// Purge completed tasks.
    Purge,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    crate::init_telemetry();
    let pool = crate::db::connect(&cli.database_url).await?;
    match cli.command {
        Command::Serve { port: _ } => crate::serve(crate::AppState { db: pool }).await?,
        Command::Purge => {
            let n = crate::purge_done(&pool).await?;
            println!("purged {n} tasks"); // stdout for program output is correct here
        }
    }
    Ok(())
}
```

## Testing

`cargo test` is the built-in runner and the only one that runs doctests; use it locally and for `--doc`. Adopt **cargo-nextest** for CI and any suite beyond a few hundred tests: it runs each test in its own process (catching leaked state and enabling per-test timeouts), parallelizes across binaries, and emits JUnit for CI dashboards.

Unit tests live in a `#[cfg(test)] mod tests` beside the code; integration tests live in `tests/`. Use `#[tokio::test]` for async tests, `rstest` for parameterized cases and fixtures, and `insta` for snapshot assertions on serialized output.

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("", false)]
    #[case("ok", true)]
    #[case("x", true)]
    fn title_parse_enforces_bounds(#[case] input: &str, #[case] valid: bool) {
        assert_eq!(Title::parse(input).is_ok(), valid);
    }

    #[tokio::test]
    async fn purge_removes_only_done() {
        // `sqlx::test` provisions an isolated database per test when configured;
        // otherwise use a transaction that is rolled back at the end.
    }

    #[test]
    fn task_dto_snapshot() {
        let dto = TaskDto { id: uuid::Uuid::nil(), title: "demo".into() };
        insta::assert_json_snapshot!(dto);
    }
}
```

SQLx provides `#[sqlx::test]`, which hands each test an isolated database (created and dropped around the test) so DB tests do not interfere—prefer it over sharing one database with manual cleanup. Use `tokio::time::pause()` (the `test-util` feature) to make time-dependent async tests deterministic instead of sleeping.

For property-based testing of parsers and invariants, add `proptest`; for benchmarking, add `criterion` in a `benches/` target. Neither belongs in a service's default dependency set—add them where they earn their place.

## Language features worth using well

- **Let-chains** (stable since 1.88, released June 26, 2025; 2024 edition only—the feature depends on the `if let` temporary-scope change that landed with that edition): chain `let` bindings and boolean tests with `&&` in `if`/`while`, flattening nested `if let`. `if let Some(user) = lookup(id) && user.active && let Ok(role) = user.role() { … }`.
- **Async closures** (stable since 1.85, released February 20, 2025, alongside the Rust 2024 edition): `async || { … }` and the `AsyncFn*` traits for higher-order async APIs.
- **Precise capturing** `use<>` (stable since 1.82, released October 17, 2024, RFC 3617): control which generics/lifetimes an `-> impl Trait` captures. In the 2024 edition, RPIT captures **all** in-scope generic parameters and lifetimes by default (so most 2021-era `use<..>` workarounds become unnecessary); write `-> impl Trait + use<>` to opt out of capturing a lifetime when you need the returned type to outlive it.
- **`unsafe extern` blocks** (2024 edition): `extern` blocks must be marked `unsafe extern { … }`, and the `unsafe_op_in_unsafe_fn` lint means an `unsafe fn` body no longer gets an implicit unsafe block—wrap unsafe operations explicitly.

Keep `unsafe` rare, localized, and documented with a `// SAFETY:` comment stating the invariant that makes it sound. Do not reach for `unsafe` to work around the borrow checker.

## Anti-patterns to avoid

| Wrong | Why it's wrong | Right |
| --- | --- | --- |
| `.unwrap()` / `.expect()` in library or request-path code | Turns a recoverable error into a process abort; a single bad input crashes the service | Return `Result`, propagate with `?`; reserve `expect` for provably-impossible cases with a message stating the invariant |
| `Arc<Mutex<T>>` everywhere to share state | Usually hides an ownership problem; contention and deadlock risk, and `std::sync::Mutex` across `.await` blocks the runtime | Restructure ownership; pass `&T`; use `tokio::sync` primitives only when state is truly shared across await points; message-pass via channels |
| CPU-bound work or blocking I/O inside `async fn` | Blocks a Tokio worker thread, starving all other tasks on it | `tokio::task::spawn_blocking` (or `rayon`) for CPU work; async-native I/O for I/O |
| `#[async_trait]` on axum extractors / handlers | Superseded—axum 0.8 and current Rust use native `async fn` in traits | Write plain `async fn` in the trait impl; drop the macro |
| Reaching for `chrono` reflexively for all date/time logic | Fine at the DB boundary, but its API invites naive-vs-aware confusion when picked out of habit | Keep it where SQLx integration is complete; for new date/time logic, `jiff` (0.2) is the better-designed default with correct DST/zone handling |
| `native-tls` / OpenSSL for TLS | Drags in a C dependency and platform build friction; reqwest 0.13 and sqlx default to rustls for a reason | Use the rustls default; enable `native-tls` only for OS-trust-store or proxy requirements |
| `Box<dyn Error>` as a library's public error type | Callers cannot match on failure kinds; loses structure | `thiserror` enum with `#[from]`/`#[source]`; `anyhow` only in application/binary code |
| `String` errors / `format!`-ed messages as control flow | No structure, no source chain, impossible to handle programmatically | Typed errors; attach human context with `anyhow::Context` at boundaries |
| Building `IN (?, ?, …)` lists for bulk queries | Each list length is a distinct prepared statement; defeats caching | Bind a Postgres array and use `= ANY($1)` / `UNNEST($1)` |
| `gen {}` / `gen fn` for iterators | Not stable on Rust 1.98—the keyword is reserved but the feature is nightly-only (feature `gen_blocks`, tracking issue #117078) | Implement `Iterator` by hand, or return `impl Iterator` from a chain of adapters |
| `.clone()` to silence a borrow-check error | Copies data to dodge a design issue; hides lifetimes | Reorder to end the borrow, pass `&T`/`&mut T`, or split the struct so fields borrow independently |
| `println!`/`eprintln!` for service diagnostics | Unstructured, unfilterable, not span-aware | `tracing` events with structured fields and a configured subscriber |

## Version & compatibility

| Component | Target | Notes / availability floor |
| --- | --- | --- |
| Rust toolchain | 1.98 (stable; 1.98.1 is the latest point release, 2026-09-03) | Language mode = Rust 2024 edition; MSRV floor for this reference is 1.98 |
| Edition | 2024 | Enables let-chains, RPIT capture rules, `unsafe extern` |
| tokio | 1.53 | Use `version = "1"`; multi-thread runtime + macros + I/O features |
| axum | 0.8 | hyper 1.0 / tower; native `async fn` traits; `{param}` path syntax; MSRV 1.80 |
| tower / tower-http | 0.5 / 0.6 | Middleware layers for tracing, timeout, compression, CORS |
| sqlx | 0.9 | `runtime-tokio` + `tls-rustls-ring` + `postgres` + `macros`; MSRV 1.94; runtime SQL must be `SqlSafeStr` |
| serde / serde_json | 1.0.229 / 1.0.151 | Derive-based; pin `version = "1"` |
| thiserror / anyhow | 2.0 / 1.0 | thiserror for libraries, anyhow for applications |
| tracing / tracing-subscriber | 0.1 / 0.3 | Use tracing-subscriber ≥ 0.3.20 (RUSTSEC-2025-0055 fix; latest 0.3.23) |
| clap | 4.6 | `derive` + `env` features |
| reqwest | 0.13 | rustls (aws-lc-rs) default; `query`/`form` now opt-in features |
| uuid / chrono | 1.x / 0.4 | `chrono` at the SQLx boundary; consider `jiff` 0.2 for new date/time logic |
| Testing | cargo-nextest, rstest 0.23, insta 1.x | nextest in CI; `cargo test --doc` for doctests |
| `gen` blocks / generators | Not available | Nightly-only on 1.98 (feature `gen_blocks`, tracking issue #117078); do not use on stable |

- **Research date:** 2026-09-05
