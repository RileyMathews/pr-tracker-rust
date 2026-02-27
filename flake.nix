{
  description = "PR tracker Rust binaries and Home Manager module";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    home-manager.url = "github:nix-community/home-manager";
    home-manager.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs =
    {
      self,
      nixpkgs,
      ...
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = self.packages.${system}.cli-tui;
          cli-tui = pkgs.callPackage ./nix/package.nix {
            binaryTargets = [ "prt" ];
          };
          all-binaries = pkgs.callPackage ./nix/package.nix { };
        }
      );

      homeManagerModules.default = import ./nix/home-manager-module.nix self;
    };
}
