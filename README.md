# pr-tracker-rust

Rust port of the original Go PR tracker.

## Binaries

- `cargo run --bin prt` (launches TUI and syncs on startup)
- `cargo run --bin prt -- <command>` (runs CLI command)

## CLI commands

- `prt auth <github-token>`
- `prt authors list|add <login>|remove <login>`
- `prt repositories list|add <owner/repo>|remove <owner/repo>`
- `prt sync`
- `prt prs`

## Environment

- `PR_TRACKER_DB` (default: `sqlite://./db.sqlite3`)

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
        }
      ];
    };
  };
}
```

Enabling `services.pr-tracker-sync` will:

- install the `prt` package in `home.packages`

### Running CLI/TUI after module install

After Home Manager installs the module/package, the binary is available on your `PATH` as:

- `prt`

Then run:

```bash
prt auth <github-token>
prt authors add <login>
prt repositories add <owner/repo>
prt
```
