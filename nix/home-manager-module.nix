self:
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.pr-tracker-sync;
in
{
  options.services.pr-tracker-sync = {
    enable = lib.mkEnableOption "PR tracker sync timer";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.cli-tui;
      defaultText = lib.literalExpression "self.packages.\${pkgs.stdenv.hostPlatform.system}.cli-tui";
      description = "Package providing the prt binary.";
    };

    dataDir = lib.mkOption {
      type = lib.types.str;
      default = "${config.xdg.dataHome}/pr-tracker-rust";
      description = "Directory where the SQLite database is stored.";
    };

    syncInterval = lib.mkOption {
      type = lib.types.str;
      default = "5m";
      example = "10m";
      description = "How frequently the sync timer runs.";
    };

    extraEnvironment = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = "Extra environment variables passed to the sync service.";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];

    systemd.user.services.pr-tracker-sync = {
      Unit = {
        Description = "Sync PR tracker data";
        After = [ "network-online.target" ];
        Wants = [ "network-online.target" ];
      };

      Service = {
        Type = "oneshot";
        Environment = [ "PR_TRACKER_DB=sqlite://${cfg.dataDir}/db.sqlite3" ] ++ lib.mapAttrsToList (name: value: "${name}=${value}") cfg.extraEnvironment;
        ExecStartPre = "${pkgs.coreutils}/bin/mkdir -p ${cfg.dataDir}";
        ExecStart = "${cfg.package}/bin/prt sync";
      };
    };

    systemd.user.timers.pr-tracker-sync = {
      Unit = {
        Description = "Run PR tracker sync every configured interval";
      };

      Timer = {
        OnBootSec = "2m";
        OnUnitActiveSec = cfg.syncInterval;
        Unit = "pr-tracker-sync.service";
        Persistent = true;
      };

      Install = {
        WantedBy = [ "timers.target" ];
      };
    };
  };
}
