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
    enable = lib.mkEnableOption "PR tracker package";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.cli-tui;
      defaultText = lib.literalExpression "self.packages.\${pkgs.stdenv.hostPlatform.system}.cli-tui";
      description = "Package providing the prt binary.";
    };

  };

  config = lib.mkIf cfg.enable {
    home.packages = [ cfg.package ];
  };
}
