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

## Nix flake + Home Manager

This repository is a flake that exposes:

- `packages.<system>.cli-tui` (builds only `cli` and `tui`)
- `packages.<system>.all-binaries` (builds all binaries)
- `homeManagerModules.default`

### Home Manager usage

```nix
{
  inputs.pr-tracker.url = "github:rileytwo/pr-tracker-rust";

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

- install the `cli`/`tui` package in `home.packages`
- create a user `systemd` service that runs `cli sync`
- create a user `systemd` timer that triggers every 5 minutes by default
