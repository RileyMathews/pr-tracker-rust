# pr-tracker-rust

Rust port of the original Go PR tracker.

## Binaries

- `cargo run --bin prt` (launches TUI)
- `cargo run --bin prt -- <command>` (runs CLI command)
- `cargo run --bin daemon`
- `cargo run --bin debug`

## CLI commands

- `prt auth <github-token>`
- `prt authors list|add <login>|remove <login>`
- `prt repositories list|add <owner/repo>|remove <owner/repo>`
- `prt sync`
- `prt prs`

## Environment

- `PR_TRACKER_DB` (default: `sqlite://./db.sqlite3`)
- `PR_TRACKER_SYNC_INTERVAL_SECONDS` (daemon only, default: `60`)

## Nix flake + Home Manager

This repository is a flake that exposes:

- `packages.<system>.cli-tui` (builds `prt`)
- `packages.<system>.all-binaries` (builds all binaries)
- `homeManagerModules.default`

### Home Manager usage

```nix
{
  inputs.pr-tracker.url = "github:rileymathews/pr-tracker-rust";

  outputs = { self, nixpkgs, home-manager, pr-tracker, ... }: {
    homeConfigurations.me = home-manager.lib.homeManagerConfiguration {
      # ...
      modules = [
        pr-tracker.homeManagerModules.default
        {
          services.pr-tracker-sync.enable = true;
          # Optional overrides:
          # services.pr-tracker-sync.syncInterval = "5m";
          # services.pr-tracker-sync.dataDir = "${config.xdg.dataHome}/pr-tracker-rust";
          # services.pr-tracker-sync.extraEnvironment = { RUST_LOG = "info"; };
        }
      ];
    };
  };
}
```

Enabling `services.pr-tracker-sync` will:

- install the `prt` package in `home.packages`
- create a user `systemd` service that runs `prt sync`
- create a user `systemd` timer that triggers every 5 minutes by default

### Running CLI/TUI after module install

After Home Manager installs the module/package, the binary is available on your `PATH` as:

- `prt`

If you are using the module-managed data directory, point interactive commands at the same DB:

```bash
export PR_TRACKER_DB="sqlite://${XDG_DATA_HOME:-$HOME/.local/share}/pr-tracker-rust/db.sqlite3"
```

Then run:

```bash
prt auth <github-token>
prt authors add <login>
prt repositories add <owner/repo>
prt sync
prt
```
