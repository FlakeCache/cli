{
  description = "FlakeCache CLI";

  nixConfig = {
    extra-substituters = [ "https://cache.centralcloud.com/default" ];
    extra-trusted-public-keys = [ "default:ESyvaQTiq681JA0iaH5tsQWS+R5qqJUVdVY1OXbi9to=" ];
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    flake-parts.url = "github:hercules-ci/flake-parts";
    crane.url = "github:ipetkov/crane";
  };

  outputs = inputs@{ flake-parts, crane, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      perSystem = { pkgs, system, ... }:
        let
          craneLib = crane.mkLib pkgs;
          cargoSrc = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              (craneLib.filterCargoSources path type)
              || builtins.baseNameOf path == "README.md";
          };

          flakecacheCli = craneLib.buildPackage {
            src = cargoSrc;
            strictDeps = true;

            nativeBuildInputs = [
              pkgs.pkg-config
            ];

            buildInputs = pkgs.lib.optionals pkgs.stdenv.isDarwin [
              pkgs.darwin.apple_sdk.frameworks.Security
              pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
            ];
          };
        in
        {
          packages = {
            default = flakecacheCli;
            "${system}" = flakecacheCli;
          };

          devShells.default = pkgs.mkShell {
            packages = [
              pkgs.cargo
              pkgs.rustc
              pkgs.rustfmt
              pkgs.clippy
              pkgs.pkg-config
            ];
          };
        };
    };
}
