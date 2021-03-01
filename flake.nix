{
  description = "automergeable";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem
      (system:
        let
          pkgs = import nixpkgs {
            overlays = [ rust-overlay.overlay ];
            system = system;
          };
          rust = pkgs.rust-bin.nightly.latest.rust;
          cargoNix = pkgs.callPackage ./Cargo.nix { };
        in
        rec
        {
          packages = {
            exp = cargoNix.workspaceMembers.exp.build;
          };

          defaultPackage = packages.exp;

          devShell = pkgs.mkShell {
            buildInputs = with pkgs;[
              (rust.override {
                extensions = [ "rust-src" "rustfmt" ];
              })
              cargo-edit
              cargo-watch
              cargo-udeps
              cargo-insta
              crate2nix
              rust-analyzer

              rnix-lsp
              nixpkgs-fmt
            ];
          };
        }
      );
}
