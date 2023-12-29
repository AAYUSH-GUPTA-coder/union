{ inputs, ... }: {
  perSystem = { devnetConfig, pkgs, lib, self', inputs', system, get-flake, ... }:
    let
      arion = inputs'.arion.packages.default;

      uniond-services = (builtins.listToAttrs (builtins.genList
        (id: {
          name = "uniond-${toString id}";
          value = import ./services/uniond.nix {
            inherit pkgs;
            inherit id;
            uniond = self'.packages.uniond;
            devnet-genesis = self'.packages.devnet-genesis;
            devnet-validator-keys = self'.packages.devnet-validator-keys;
            devnet-validator-node-ids = self'.packages.devnet-validator-node-ids;
          };
        })
        devnetConfig.validatorCount));

      simd-services = (builtins.listToAttrs (builtins.genList
        (id: {
          name = "simd-${toString id}";
          value = import ./services/simd.nix {
            inherit pkgs;
            inherit id;
            simd = self'.packages.simd;
            simd-genesis = self'.packages.simd-genesis;
            simd-validator-keys = self'.packages.simd-validator-keys;
            simd-validator-node-ids = self'.packages.simd-validator-node-ids;
          };
        })
        devnetConfig.validatorCount));

      uniond-testnet-genesis-services = (builtins.listToAttrs (builtins.genList
        (id: {
          name = "uniond-${toString id}";
          value = import ./services/unionvisor.nix {
            inherit pkgs;
            inherit id;
            uniond = (get-flake inputs.v0_15_0).packages.${system}.uniond;
            unionvisor = self'.packages.unionvisor;
            devnet-genesis = self'.packages.minimal-genesis;
            devnet-validator-keys = self'.packages.minimal-validator-keys;
            devnet-validator-node-ids = self'.packages.minimal-validator-node-ids;
            network = "union-minimal-1";
            bundle = self'.packages.bundle-testnet-next;
          };
        })
        4));

      sepolia-services = {
        geth = import ./services/geth.nix {
          inherit pkgs;
          config = self'.packages.devnet-eth-config;
        };
        lodestar = import ./services/lodestar.nix {
          inherit pkgs;
          config = self'.packages.devnet-eth-config;
          validatorCount = devnetConfig.ethereum.beacon.validatorCount;
        };
      };

      postgres-services = {
        postgres = import ./services/postgres.nix { inherit lib pkgs; };
      };

      # hasura-services = import ./services/hasura.nix {
      #   inherit lib pkgs;
      # };
      # hubble-services = { hubble = import ./services/hubble.nix { inherit lib; image = self'.packages.hubble-image; }; };

      devnet = {
        project.name = "devnet";
        services = sepolia-services // uniond-services // postgres-services;
      };

      devnet-minimal = {
        project.name = "devnet-minimal";
        services = uniond-testnet-genesis-services;
      };

      union = {
        project.name = "union";
        services = uniond-services;
      };

      sepolia = {
        project.name = "sepolia";
        services = sepolia-services;
      };

      spec = {
        modules = [ (devnet // { networks.union-devnet = { }; }) ];
      };

      spec-union = {
        modules = [ (union // { networks.union-devnet = { }; }) ];
      };

      spec-simd = {
        modules = [{
          project.name = "simd-devnet";
          networks.simd-devnet = { };
          services = simd-services;
        }];
      };

      spec-eth = {
        modules = [{
          project.name = "union-devnet-eth";
          networks.union-devnet = { };
          services = sepolia-services;
        }];
      };

      build = arion.build spec;

      build-eth = arion.build spec-eth;

      build-union = arion.build spec-union;

      build-simd = arion.build spec-simd;

      build-voyager-queue = arion.build {
        modules = [{
          project.name = "postgres";
          services = postgres-services;
        }];
      };
    in
    {
      packages.devnet =
        pkgs.writeShellApplication {
          name = "union-devnet";
          runtimeInputs = [ arion ];
          text = ''
            arion --prebuilt-file ${build} up --build --force-recreate -V --always-recreate-deps --remove-orphans
          '';
        };

      packages.devnet-simd =
        pkgs.writeShellApplication {
          name = "simd-devnet";
          runtimeInputs = [ arion ];
          text = ''
            arion --prebuilt-file ${build-simd} up --build --force-recreate -V --always-recreate-deps --remove-orphans
          '';
        };

      packages.devnet-eth =
        pkgs.writeShellApplication {
          name = "union-devnet-eth";
          runtimeInputs = [ arion ];
          text = ''
            arion --prebuilt-file ${build-eth} up --build --force-recreate -V --always-recreate-deps --remove-orphans
          '';
        };

      packages.devnet-union =
        pkgs.writeShellApplication {
          name = "union-devnet-union";
          runtimeInputs = [ arion ];
          text = ''
            arion --prebuilt-file ${build-union} up --build --force-recreate -V --always-recreate-deps --remove-orphans
          '';
        };

      packages.voyager-queue =
        pkgs.writeShellApplication {
          name = "postgres";
          runtimeInputs = [ arion ];
          text = ''
            arion --prebuilt-file ${build-voyager-queue} up --build --force-recreate -V --always-recreate-deps --remove-orphans
          '';
        };

      _module.args.networks = {
        inherit devnet devnet-minimal union sepolia;
      };
    };
}
