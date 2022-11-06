{
  description = "automergeable";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    crane.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
    flake-utils,
    crane,
  }:
    flake-utils.lib.eachDefaultSystem
    (
      system: let
        pkgs = import nixpkgs {
          overlays = [rust-overlay.overlays.default];
          inherit system;
        };
        rust = pkgs.rust-bin.stable.latest.default;
        craneLib = crane.lib.${system};
        commonArgs = {
          src = craneLib.cleanCargoSource ./.;
        };
        cargoArtifacts = craneLib.buildDepsOnly (
          commonArgs
          // {
            pname = "exp-deps";
          }
        );
        exp = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;
            cargoTestCommand = "echo skipping tests";
          }
        );
      in rec
      {
        packages = {
          inherit exp;
          default = exp;
        };

        checks = {
          inherit exp;
        };

        formatter = pkgs.alejandra;

        devShell = pkgs.mkShell {
          buildInputs = with pkgs; [
            rust
            cargo-watch
            cargo-udeps
            cargo-insta
            rust-analyzer
            cargo-outdated

            rnix-lsp
          ];
        };
      }
    );
}
