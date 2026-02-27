# pr-tracker-rust

Rust port of the original Go PR tracker.

## Binaries

- `cargo run --bin cli -- <command>`
- `cargo run --bin tui`
- `cargo run --bin daemon`
- `cargo run --bin debug`

## CLI commands

- `auth <github-token>`
- `authors list|add <login>|remove <login>`
- `repositories list|add <owner/repo>|remove <owner/repo>`
- `sync`
- `prs`

## Environment

- `PR_TRACKER_DB` (default: `sqlite://./db.sqlite3`)
- `PR_TRACKER_SYNC_INTERVAL_SECONDS` (daemon only, default: `60`)
