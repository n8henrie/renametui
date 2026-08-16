{
  description = "Development environment and package for renametui";
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { self, nixpkgs, ... }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];
      eachSystem =
        with nixpkgs.lib;
        f: foldAttrs mergeAttrs { } (map (s: mapAttrs (_: v: { ${s} = v; }) (f s)) systems);
    in
    eachSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        formatter = pkgs.nixfmt;
        packages =
          let
            renametui = pkgs.callPackage ./package.nix { };
          in
          {
            inherit renametui;
            default = renametui;
          };
        devShells.default = pkgs.mkShellNoCC {
          inputsFrom = [ self.outputs.packages.renametui ];
          packages = [
            pkgs.clippy
            pkgs.deadnix
            pkgs.nixfmt
            pkgs.rust-analyzer
            pkgs.rustfmt
            pkgs.statix
          ];
          RUST_BACKTRACE = "1";
        };
      }
    );
}
