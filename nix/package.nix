{
  lib,
  rustPlatform,
  pkg-config,
  openssl,
  sqlite,
  stdenv,
  darwin,
  binaryTargets ? [ ],
}:
rustPlatform.buildRustPackage {
  pname = "pr-tracker-rust";
  version = "0.1.0";

  src = lib.cleanSource ../.;

  cargoLock = {
    lockFile = ../Cargo.lock;
  };

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ openssl sqlite ] ++ lib.optionals stdenv.isDarwin [ darwin.apple_sdk.frameworks.Security ];

  cargoBuildFlags = lib.optionals (binaryTargets != [ ]) (
    lib.concatMap (bin: [
      "--bin"
      bin
    ]) binaryTargets
  );

  meta = {
    description = "Track pull requests across repositories";
    homepage = "https://github.com/rileytwo/pr-tracker-rust";
    license = lib.licenses.mit;
    mainProgram = "cli";
    platforms = lib.platforms.all;
  };
}
