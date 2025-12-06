{
  description = "FlakeCache CLI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
  };

  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
        "x86_64-windows"
      ];

      perSystem = { config, self', inputs', pkgs, system, ... }: let
        rust = pkgs.rust-bin.stable.latest.default;
        craneLib = inputs.crane.mkLib pkgs;
        mkPackage = crossSystem: let
          craneLibCross = if crossSystem != null then craneLib.overrideToolchain (pkgs.pkgsCross.${crossSystem}.rust-bin.stable.latest.default) else craneLib;
        in craneLibCross.buildPackage {
          src = ./.;
          nativeBuildInputs = with pkgs; [
            pkg-config
          ];
          buildInputs = with pkgs; [
            openssl
          ] ++ lib.optionals stdenv.isDarwin [
            darwin.apple_sdk.frameworks.Security
            darwin.apple_sdk.frameworks.SystemConfiguration
          ];
        };
      in {
        packages = {
          default = mkPackage null;  # Native build
        } // (if system == "x86_64-linux" then {
          "x86_64-linux" = mkPackage null;
          "aarch64-linux" = mkPackage "aarch64-multiplatform";
        } else if system == "aarch64-darwin" then {
          "x86_64-darwin" = mkPackage null;
          "aarch64-darwin" = mkPackage null;
        } else if system == "x86_64-windows" then {
          "x86_64-windows" = mkPackage null;
        } else {});

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            rust
            cargo
            rustc
          ];
        };
      };
    };
}