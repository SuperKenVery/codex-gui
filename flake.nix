{
  description = "GPUI desktop shell for codex-gui";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, crane, rust-overlay, ... }:
    let
      systems = [ "aarch64-darwin" "x86_64-darwin" "x86_64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      perSystem = system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          lib = pkgs.lib;
          rustToolchain = pkgs.rust-bin.stable.latest.default.override {
            extensions = [ "rust-src" ];
          };
          craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;
          commonArgs = {
            pname = "codex-gui";
            version = "0.1.0";
            src = craneLib.cleanCargoSource ./.;
            strictDeps = true;
            buildInputs = lib.optionals pkgs.stdenv.isDarwin [
              pkgs.apple-sdk
              pkgs.libiconv
            ];
          };
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          codex-gui = craneLib.buildPackage (commonArgs // {
            inherit cargoArtifacts;
          });
        in
        {
          packages = {
            default = codex-gui;
            inherit codex-gui;
          };

          checks = {
            clippy = craneLib.cargoClippy (commonArgs // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            });
            fmt = craneLib.cargoFmt {
              src = commonArgs.src;
            };
          };

          devShells.default = craneLib.devShell {
            checks = self.checks.${system};
            env = {
              RUST_BACKTRACE="1";
            };
          };
        };
    in
    {
      packages = forAllSystems (system: (perSystem system).packages);
      checks = forAllSystems (system: (perSystem system).checks);
      devShells = forAllSystems (system: (perSystem system).devShells);
    };
}
