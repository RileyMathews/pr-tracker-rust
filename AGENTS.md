# AGENTS.md — pr-tracker-rust

## Project Overview

A Rust CLI/TUI tool that gives developers a quick overview of PRs needing their
attention. It syncs with GitHub's API, scores PRs by importance, and presents
them in a terminal UI or CLI. The binary entry point is `prt`.

Architecture follows **functional core / imperative shell**: pure business logic
(models, scoring, diffing, TUI state) is strictly separated from I/O (database,
HTTP, terminal). This makes the core exhaustively testable without mocks.

## Build / Lint / Test Commands

```sh
# Full verification (what CI and agents should run)
just verify              # clippy -D warnings → fmt --check → cargo check

# Build
cargo build              # debug build
cargo build --release    # release build

# Lint
cargo clippy -- -D warnings   # all warnings are errors
cargo fmt --check              # check formatting (default rustfmt settings)
cargo fmt                      # auto-format

# Test — all tests
cargo test

# Test — single test by name (substring match)
cargo test <test_name>
# Example: cargo test classifies_new_pr

# Test — all tests in a specific module
cargo test --lib core::tests
cargo test --lib scoring::tests
cargo test --lib models::tests
cargo test --lib tui::state::tests

# Run binaries
cargo run --bin prt              # main entry (TUI with no args, CLI with args)
cargo run --bin prt -- <command> # CLI mode
cargo run --bin daemon           # background sync loop
```

The `just agent-full-verify` target is an alias for `just verify` — use it as
a final gate before committing.

## Project Structure

```
src/
  lib.rs              # crate root, re-exports all modules
  main.rs             # stub ("Use one of the binaries")
  models.rs           # domain types: PullRequest, PrComment, CiStatus, etc.
  core.rs             # pure sync-diff algorithm (no I/O)
  scoring.rs          # pure PR importance scoring (no I/O)
  db.rs               # DatabaseRepository (SQLite via sqlx)
  service.rs          # bridges GitHub API → domain models
  sync.rs             # orchestrates full sync across tracked repos
  cli_app.rs          # CLI command handling (clap derive)
  github/
    mod.rs            # GitHubClient (reqwest HTTP)
    graphql.rs        # GraphQL queries and response types
    schema.rs         # REST API response types
  tui/
    mod.rs            # re-exports app::run
    app.rs            # TUI main loop (crossterm + ratatui)
    state.rs          # SharedState, pure utility fns
    navigation.rs     # Screen/ViewMode/AuthorsPane enums
    widgets.rs        # reusable widget helpers (badges, styles)
    tasks.rs          # background task spawning
    pr_list/          # PR list screen (state / events / render)
    authors/          # authors screen  (state / events / render)
src/bin/
  prt.rs              # main binary — TUI (no args) or CLI (with args)
  cli.rs, tui.rs      # standalone CLI / TUI entry points
  daemon.rs           # periodic background sync
  debug.rs            # dev harness
migrations/           # SQLite migrations (sqlx, 000001–000006)
```

## Architecture: Functional Core / Imperative Shell

**Pure (core) modules** — synchronous, zero I/O, heavily tested:
- `models.rs` — domain types + behavior (`is_acknowledged`, `all_changes`, etc.)
- `core.rs` — sync diff algorithm (classifies PRs as new/updated/removed)
- `scoring.rs` — importance scoring
- `tui/state.rs`, `tui/widgets.rs`, `tui/navigation.rs` — TUI state & helpers
- `tui/pr_list/state.rs`, `tui/authors/state.rs` — screen-specific state

**Impure (shell) modules** — perform I/O, no unit tests (tested via integration):
- `db.rs`, `github/`, `sync.rs`, `service.rs`, `cli_app.rs`
- `tui/app.rs`, `tui/*/events.rs`, `tui/*/render.rs`, `tui/tasks.rs`

When adding logic, put it in a pure module and test it there. Shell modules
should be thin wrappers that call pure functions and perform I/O.

## Code Style Guidelines

### Formatting
- Default `rustfmt` settings (no `rustfmt.toml`). Run `cargo fmt`.
- Clippy with `-D warnings` — all lints are errors.

### Imports
Three groups separated by blank lines:
```rust
use std::collections::HashMap;       // 1. std

use chrono::{DateTime, Utc};         // 2. external crates
use tokio::sync::Semaphore;

use crate::models::PullRequest;      // 3. crate-internal
use crate::core::SyncDiff;
```
Multiple items from the same path use braces: `use crate::models::{A, B, C};`
Different paths get separate `use` lines (don't deeply nest).

### Naming
- Modules: `snake_case` (e.g., `cli_app`, `pr_list`)
- Types/Enums: `PascalCase` (e.g., `PullRequest`, `CiStatus::Success`)
- Functions: `snake_case` (e.g., `importance_score`, `process_pull_request_sync_results`)
- Constants: `SCREAMING_SNAKE_CASE` (e.g., `MAX_CONCURRENT_REPOS`)
- Tests: descriptive `snake_case` (e.g., `author_scores_higher_than_non_author`)
- DB→domain conversions: `into_model()` method on row structs

### Error Handling
- Use `anyhow::Result<T>` for all fallible functions.
- `anyhow::bail!("message")` for early error returns.
- `?` operator for propagation.
- Add context in bail messages or `.map_err()` closures.
- No custom error types — this project uses `anyhow` exclusively (not `thiserror`).

### Types & Data
- Domain structs: `#[derive(Debug, Clone, PartialEq, Eq)]` with `pub` fields.
- Timestamps: `DateTime<Utc>` in Rust, stored as Unix integers in SQLite.
- Enums: manual `as_i64()` / `from_i64()` for DB serialization.
- DB row structs: `#[derive(FromRow)]` → convert via `into_model()`.
- Collections of identifiers: `Vec<String>` (stored as JSON in SQLite).
- No trait objects or dynamic dispatch — everything is static/monomorphic.

### Async
- `#[tokio::main]` with multi-thread runtime on all binary entry points.
- Concurrent work via `JoinSet` + `Semaphore` (see `sync.rs`).
- Background tasks communicate via `mpsc::UnboundedSender`.

### Testing Conventions
- All tests are inline `#[cfg(test)] mod tests` blocks (no `tests/` directory).
- Use helper factory functions (`test_pr()`, `empty_pr()`, `build_pull_request()`).
- Organize tests with section comments (e.g., `// -- Ownership axis --`).
- `build_pull_request()` in `models.rs` uses a builder pattern with `TestPrEvent`
  enum for readable, composable test setup.
- Only test pure modules — impure code is not unit-tested.
- `pretty_assertions` is available as a dev dependency if needed.

### TUI Screen Pattern
Each TUI screen follows a 4-file convention:
```
tui/<screen>/
  mod.rs      — pub mod + re-exports
  state.rs    — screen state struct (pure, tested)
  events.rs   — key event handler (impure)
  render.rs   — draw function (impure)
```

### Database
- Single `DatabaseRepository` wrapping `SqlitePool`.
- Raw SQL with `sqlx::query` / `sqlx::query_as` (no ORM).
- Upserts via `INSERT ... ON CONFLICT DO UPDATE`.
- Migrations in `migrations/` directory (auto-run by sqlx).

## Environment

- `PR_TRACKER_DB` env var points to the SQLite database path.
- `GITHUB_TOKEN` env var is required for GitHub API access.
- Dev shell via Nix flake (`nix develop` or `direnv allow`).
- Nix provides: `rustc`, `cargo`, `clippy`, `rustfmt`, `rust-analyzer`, `sqlite`.
