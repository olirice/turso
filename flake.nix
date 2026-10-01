{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    # PostgreSQL 18.6, the build pg/head's capture and recorded transcripts
    # came from. Pinned apart from nixpkgs so a toolchain bump never
    # changes pg_dump's output under the pg_dump round-trip tests.
    nixpkgs-postgres.url = "github:nixos/nixpkgs/f45c6f04c2f013f004bf94e284e95d72898d9393";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, nixpkgs-postgres, flake-utils, rust-overlay, crane, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        toolchain = (pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml).override {
          extensions = [ "rust-analyzer" "rust-src" ];
          targets = [ "wasm32-unknown-unknown" ];
        };

        lib = pkgs.lib;

        postgres = (import nixpkgs-postgres { inherit system; }).postgresql_18;

        # Custom SQLite package with debug enabled
        sqlite-debug = pkgs.sqlite.overrideAttrs (oldAttrs: rec {
          name = "sqlite-debug-${oldAttrs.version}";
          configureFlags = oldAttrs.configureFlags ++ [ "--enable-debug" ];
          dontStrip = true;
          separateDebugInfo = true;
        });

        cargoArtifacts = craneLib.buildDepsOnly {
          src = ./.;
          pname = "turso";
          nativeBuildInputs = with pkgs; [ python3 ];
        };

        commonArgs = {
          inherit cargoArtifacts;
          pname = "turso";
          src = ./.;
          nativeBuildInputs = with pkgs; [ python3 ];
          strictDeps = true;
        };

        craneLib = ((crane.mkLib pkgs).overrideToolchain toolchain);
      in
      rec {
        formatter = pkgs.nixpkgs-fmt;
        checks = {
          doc = craneLib.cargoDoc commonArgs;
          fmt = craneLib.cargoFmt commonArgs;
          clippy = craneLib.cargoClippy (commonArgs // {
            # TODO: maybe add `-- --deny warnings`
            cargoClippyExtraArgs = "--all-targets";
          });
        };
        packages.turso_cli = craneLib.buildPackage (commonArgs // {
          cargoExtraArgs = "--package turso_cli";
        });
        packages.default = packages.turso_cli;
        packages.postgres = postgres;
        devShells.default = with pkgs; mkShell {
          nativeBuildInputs = [
            clang
            sqlite-debug  # Use debug-enabled SQLite
            gnumake
            tcl
            python3
            nodejs
            toolchain
            uv
            postgres
          ] ++ lib.optionals pkgs.stdenv.isDarwin [
            apple-sdk
          ];
        };
        devShells.fuzz = with pkgs; mkShell {
          nativeBuildInputs = [
            (pkgs.rust-bin.selectLatestNightlyWith (toolchain: toolchain.minimal))
          ] ++ lib.optionals pkgs.stdenv.isDarwin [
            apple-sdk
          ];
        };
      }
    );
}
