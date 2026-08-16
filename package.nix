{
  lib,
  rustPlatform,
}:

rustPlatform.buildRustPackage {
  pname = "renametui";
  version = "0.1.0";

  src = lib.cleanSource ./.;

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  strictDeps = true;

  meta = {
    description = "Conflict-aware terminal UI for regex-based file and directory renaming";
    license = lib.licenses.mit;
    mainProgram = "renametui";
    platforms = lib.platforms.unix;
  };
}
