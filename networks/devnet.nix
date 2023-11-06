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

      uniond-testnet-genesis-services = (builtins.listToAttrs (builtins.genList
        (id: {
          name = "uniond-${toString id}";
          value = import ./services/unionvisor.nix {
            inherit pkgs;
            inherit id;
            uniond = (get-flake inputs.v0_8_0).packages.${system}.uniond;
            unionvisor = self'.packages.unionvisor;
            devnet-genesis = self'.packages.minimal-genesis;
            devnet-validator-keys = self'.packages.minimal-validator-keys;
            devnet-validator-node-ids = self'.packages.minimal-validator-node-ids;
            network = "union-minimal-1";
            bundle = self'.packages.bundle-testnet-3;
          };
        })
        4));

       wasmd-services = (builtins.listToAttrs (builtins.genList
        (id: {
          name = "wasmd-${toString id}";
          value = import ./services/wasmd.nix {
            inherit pkgs;
            inherit id;
            wasmd = self'.packages.wasmd;
            devnet-genesis = self'.packages.wasmd-genesis;
            devnet-validator-keys = self'.packages.wasmd-validator-keys;
            devnet-validator-node-ids = self'.packages.wasmd-validator-node-ids;
          };
        })
        4));

      sepolia-services = {
        geth = import ./services/geth.nix {
          inherit pkgs;
          config = self'.packages.devnet-evm-config;
        };
        lodestar = import ./services/lodestar.nix {
          inherit pkgs;
          config = self'.packages.devnet-evm-config;
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

      spec-cosmos = {
        modules = [ (union // { networks.union-devnet = { }; }) ];
      };

      spec-evm = {
        modules = [{
          project.name = "union-devnet-evm";
          networks.union-devnet = { };
          services = sepolia-services;
        }];
      };

     spec-wasmd = {
        modules = [{
          project.name = "wasmd-devnet";
          networks.wasmd-devnet = { };
          services = wasmd-services;
        }];
      };

      build = arion.build spec;

      build-evm = arion.build spec-evm;

      build-cosmos = arion.build spec-cosmos;

      build-wasmd = arion.build spec-wasmd;

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

      packages.devnet-evm =
        pkgs.writeShellApplication {
          name = "union-devnet-evm";
          runtimeInputs = [ arion ];
          text = ''
            arion --prebuilt-file ${build-evm} up --build --force-recreate -V --always-recreate-deps --remove-orphans
          '';
        };

      packages.devnet-cosmos =
        pkgs.writeShellApplication {
          name = "union-devnet-cosmos";
          runtimeInputs = [ arion ];
          text = ''
            arion --prebuilt-file ${build-cosmos} up --build --force-recreate -V --always-recreate-deps --remove-orphans
          '';
        };

      packages.devnet-wasmd =
        pkgs.writeShellApplication {
          name = "wasmd-devnet";
          runtimeInputs = [ arion ];
          text = ''
            arion --prebuilt-file ${build-wasmd} up --build --force-recreate -V --always-recreate-deps --remove-orphans
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
