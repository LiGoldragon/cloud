{
  description = "cloud - Criome cloud provider API daemon and thin CLI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-build = {
      url = "github:LiGoldragon/rust-build";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-build,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
        rust = rust-build.lib.${system}.fromPkgs pkgs;

        inherit (rust) craneLib toolchain;
        schemaFilter = path: type: type == "regular" && pkgs.lib.hasSuffix ".schema" path;
        src = rust.cleanSource {

          root = ./.;

          extraFilters = [ schemaFilter ];

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
              --run 'CF_API_TOKEN=$(${pkgs.gopass}/bin/gopass show -o cloudflare.com/token) || { echo "cloud: cannot fetch CF_API_TOKEN from gopass cloudflare.com/token" >&2; exit 78; }; export CF_API_TOKEN'
          '';
        };
        # Hetzner Phase 1 reads the REST API in-process, so unlike flarectl the
        # daemon never shells out to a Hetzner CLI for create/observe/destroy.
        # The shim injects HCLOUD_TOKEN from gopass and keeps the hcloud CLI on
        # PATH for operator debugging.
        hetznerCli = pkgs.symlinkJoin {
          name = "hcloud-gopass-wrapped";
          paths = [ pkgs.hcloud ];
          nativeBuildInputs = [ pkgs.makeWrapper ];
          postBuild = ''
            wrapProgram $out/bin/hcloud \
              --run 'HCLOUD_TOKEN=$(${pkgs.gopass}/bin/gopass show -o hetzner/api-token) || { echo "cloud: cannot fetch HCLOUD_TOKEN from gopass hetzner/api-token" >&2; exit 78; }; export HCLOUD_TOKEN'
          '';
        };
        # DigitalOcean Phase 1 reads the REST API in-process, like Hetzner. The
        # shim injects DIGITALOCEAN_ACCESS_TOKEN from gopass and keeps the doctl
        # CLI on PATH for operator debugging.
        digitaloceanCli = pkgs.symlinkJoin {
          name = "doctl-gopass-wrapped";
          paths = [ pkgs.doctl ];
          nativeBuildInputs = [ pkgs.makeWrapper ];
          postBuild = ''
            wrapProgram $out/bin/doctl \
              --run 'DIGITALOCEAN_ACCESS_TOKEN=$(${pkgs.gopass}/bin/gopass show -o digitalocean.com/api-token) || { echo "cloud: cannot fetch DIGITALOCEAN_ACCESS_TOKEN from gopass digitalocean.com/api-token" >&2; exit 78; }; export DIGITALOCEAN_ACCESS_TOKEN'
          '';
        };
        cloudRuntimePath = pkgs.lib.makeBinPath [
          cloudflareCli
          hetznerCli
          digitaloceanCli
        ];
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        digitaloceanCargoExtraArgs = "--features digitalocean,cloudflare";
        digitaloceanCargoArtifacts = craneLib.buildDepsOnly (
          commonArgs
          // {
            cargoExtraArgs = digitaloceanCargoExtraArgs;
          }
        );
        cloudPackage =
          packageArguments:
          craneLib.buildPackage (
            commonArgs
            // packageArguments
            // {
              nativeBuildInputs = [ pkgs.makeWrapper ];
              meta.mainProgram = "cloud";
              postInstall = ''
                wrapProgram $out/bin/cloud-daemon --prefix PATH : ${cloudRuntimePath} \
                  --run 'export HCLOUD_TOKEN=''${HCLOUD_TOKEN:-$(${pkgs.gopass}/bin/gopass show -o hetzner/api-token 2>/dev/null)}' \
                  --run 'export DIGITALOCEAN_ACCESS_TOKEN=''${DIGITALOCEAN_ACCESS_TOKEN:-$(${pkgs.gopass}/bin/gopass show -o digitalocean.com/api-token 2>/dev/null)}' \
                  --run 'export CLOUDFLARE_DNS_TOKEN=''${CLOUDFLARE_DNS_TOKEN:-$(${pkgs.gopass}/bin/gopass show -o cloudflare.com/token 2>/dev/null)}'
              '';
            }
          );
        digitaloceanLiveTest = pkgs.writeShellApplication {
          name = "cloud-digitalocean-live-test";
          runtimeInputs = [
            pkgs.gopass
            pkgs.openssh
            toolchain
          ];
          text = ''
            if [ ! -f Cargo.toml ]; then
              echo "cloud: run this live test app from the cloud repository root" >&2
              exit 2
            fi
            if [ -z "''${DIGITALOCEAN_ACCESS_TOKEN:-}" ]; then
              DIGITALOCEAN_ACCESS_TOKEN=$(gopass show -o digitalocean.com/api-token)
              export DIGITALOCEAN_ACCESS_TOKEN
            fi
            cargo test --features digitalocean --test digitalocean_live -- --ignored --nocapture
          '';
        };
      in
      {
        packages = {
          default = cloudPackage { inherit cargoArtifacts; };
          digitalocean = cloudPackage {
            cargoArtifacts = digitaloceanCargoArtifacts;
            cargoExtraArgs = digitaloceanCargoExtraArgs;
            pname = "cloud-digitalocean";
          };
        };

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

          digitalocean-test = craneLib.cargoTest (
            commonArgs
            // {
              cargoArtifacts = digitaloceanCargoArtifacts;
              cargoTestExtraArgs = "--features digitalocean,cloudflare --test digitalocean";
            }
          );

          digitalocean-live-test-compiles = craneLib.cargoTest (
            commonArgs
            // {
              cargoArtifacts = digitaloceanCargoArtifacts;
              cargoTestExtraArgs = "--features digitalocean --test digitalocean_live -- --ignored --list";
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

          digitalocean-clippy = craneLib.cargoClippy (
            commonArgs
            // {
              cargoArtifacts = digitaloceanCargoArtifacts;
              cargoClippyExtraArgs = "--features digitalocean,cloudflare --all-targets -- -D warnings";
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

        apps.daemon-digitalocean = {
          type = "app";
          program = "${self.packages.${system}.digitalocean}/bin/cloud-daemon";
        };

        apps.digitalocean-live-test = {
          type = "app";
          program = "${digitaloceanLiveTest}/bin/cloud-digitalocean-live-test";
        };

        apps.meta = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/meta-cloud";
        };

        devShells.default = pkgs.mkShell {
          name = "cloud";
          packages = [
            cloudflareCli
            hetznerCli
            digitaloceanCli
            pkgs.jujutsu
            toolchain
          ];
        };
      }
    );
}
