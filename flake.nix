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
        src = craneLib.cleanCargoSource ./.;
        commonArgs = {
          inherit src;
          strictDeps = true;
        };
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        # Cloudflare CLI runtime dependency. The `cloudflare` Cargo
        # feature (default) shells out to flarectl from the
        # cloudflare_cli adapter; wrapping cloud-daemon with flarectl
        # on PATH means the daemon never relies on whatever happens to
        # be in the user profile. Per psyche 2026-05-27 (spirit 923).
        #
        # The flarectl binary is itself wrapped so that CF_API_TOKEN
        # is populated from gopass at `cloudflare/api-token` before
        # exec. This realises the FEMOS env-var-populated-by-password-
        # manager auth pattern (spirit 682, 689, 924) end-to-end
        # inside the nix closure — no human-driven env-var management
        # at daemon start.
        cloudflareCli = pkgs.symlinkJoin {
          name = "flarectl-gopass-wrapped";
          paths = [ pkgs.flarectl ];
          nativeBuildInputs = [ pkgs.makeWrapper ];
          postBuild = ''
            wrapProgram $out/bin/flarectl \
              --run 'export CF_API_TOKEN=$(${pkgs.gopass}/bin/gopass show -o cloudflare/api-token)'
          '';
        };
        cloudRuntimePath = pkgs.lib.makeBinPath [ cloudflareCli ];
      in
      {
        packages.default = craneLib.buildPackage (
          commonArgs
          // {
            inherit cargoArtifacts;
            nativeBuildInputs = [ pkgs.makeWrapper ];
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
