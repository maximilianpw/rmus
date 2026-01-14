{
  description = "rmus - Music Player TUI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    crane,
    flake-utils,
    rust-overlay,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [(import rust-overlay)];
      };

      rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

      commonArgs = {
        src = craneLib.cleanCargoSource ./.;
        strictDeps = true;

        nativeBuildInputs = with pkgs; [pkg-config];
        buildInputs = with pkgs; [
          openssl
          mpv
        ];

        OPENSSL_DIR = "${pkgs.openssl.dev}";
        OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
      };

      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      rmus = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
      });
    in {
      checks = {
        inherit rmus;
      };

      packages = {
        default = rmus;
      };

      apps.default = flake-utils.lib.mkApp {
        drv = rmus;
      };

      devShells.default = craneLib.devShell {
        checks = self.checks.${system};
        packages = with pkgs; [
          mpv
        ];

        OPENSSL_DIR = "${pkgs.openssl.dev}";
        OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
      };
    });
}
