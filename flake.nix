{
  description = "cloud - Criome cloud provider API daemon and thin CLI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      fenix,
      crane,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        toolchain = fenix.packages.${system}.stable.withComponents [
          "cargo"
          "rustc"
          "rustfmt"
          "clippy"
          "rust-analyzer"
          "rust-src"
        ];
        craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
        schemaFilter = path: type: type == "regular" && pkgs.lib.hasSuffix ".schema" path;
        sourceFilter =
          path: type:
          type == "directory" || (craneLib.filterCargoSources path type) || (schemaFilter path type);
        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter = sourceFilter;
          name = "source";
        };
        commonArgs = {
          inherit src;
          strictDeps = true;
        };
        cloudflareCli = pkgs.symlinkJoin {
          name = "flarectl-gopass-wrapped";
          paths = [ pkgs.flarectl ];
          nativeBuildInputs = [ pkgs.makeWrapper ];
          postBuild = ''
            wrapProgram $out/bin/flarectl \
              --run 'CF_API_TOKEN=$(${pkgs.gopass}/bin/gopass show -o cloudflare/api-token) || { echo "cloud: cannot fetch CF_API_TOKEN from gopass cloudflare/api-token" >&2; exit 78; }; export CF_API_TOKEN'
          '';
        };
        cloudRuntimePath = pkgs.lib.makeBinPath [ cloudflareCli ];
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;
      in
      {
        packages.default = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;
            nativeBuildInputs = [ pkgs.makeWrapper ];
            meta.mainProgram = "cloud";
            postInstall = ''
              wrapProgram $out/bin/cloud-daemon --prefix PATH : ${cloudRuntimePath}
            '';
          }
        );

        checks = {
          build = craneLib.cargoBuild (
            commonArgs
            // {
              inherit cargoArtifacts;
            }
          );

          test = craneLib.cargoTest (
            commonArgs
            // {
              inherit cargoArtifacts;
            }
          );

          fmt = craneLib.cargoFmt {
            inherit src;
          };

          clippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- -D warnings";
            }
          );
        };

        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/cloud";
        };

        apps.daemon = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/cloud-daemon";
        };

        apps.meta = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/meta-cloud";
        };

        devShells.default = pkgs.mkShell {
          name = "cloud";
          packages = [
            cloudflareCli
            pkgs.jujutsu
            toolchain
          ];
        };
      }
    );
}
