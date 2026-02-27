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

      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShell {
            nativeBuildInputs = [ pkgs.pkg-config ];
            buildInputs =
              [
                pkgs.rustc
                pkgs.cargo
                pkgs.clippy
                pkgs.rustfmt
                pkgs.rust-analyzer
                pkgs.openssl
                pkgs.sqlite
              ]
              ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
                pkgs.darwin.apple_sdk.frameworks.Security
              ];
          };
        }
      );

      homeManagerModules.default = import ./nix/home-manager-module.nix self;
    };
}
