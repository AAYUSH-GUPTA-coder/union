{ pkgs, simd, simd-genesis, simd-validator-keys, simd-validator-node-ids, id }:
let
  portIncrease = 100;
  getNodeID = nodeFile:
    pkgs.runCommand "get-node-id" { } ''
      export HOME=$(pwd)
      ${simd}/bin/wasmd init testnet --home .
      cp ${simd-validator-node-ids}/${nodeFile} ./config/node_key.json
      NODE_ID=$(${simd}/bin/wasmd tendermint show-node-id --home .)
      echo -n $NODE_ID > $out
    '';

  seedNode = builtins.readFile (getNodeID "valnode-0.json");
  # All nodes connect to node 0
  params = if id == 0 then "" else "--p2p.persistent_peers ${seedNode}@simd-0:26656";
in
{
  image = {
    enableRecommendedContents = true;
    contents = [
      pkgs.coreutils
      simd-genesis
      simd
      simd-validator-keys
      simd-validator-node-ids
    ];
  };
  service = {
    tty = true;
    stop_signal = "SIGINT";
    ports = [
      # CometBFT JSONRPC 26657
      "${toString (26657 + portIncrease + id)}:26657"
      # Cosmos SDK GRPC 9090
      "${toString (9090 + portIncrease + id)}:9090"
      # Cosmos SDK REST 1317
      "${toString (1317 + portIncrease + id)}:1317"
    ];
    command = [
      "sh"
      "-c"
      ''
        cp -R ${simd-genesis} .
        cp ${simd-validator-keys}/valkey-${toString id}.json ./config/priv_validator_key.json
        cp ${simd-validator-node-ids}/valnode-${toString id}.json ./config/node_key.json
        echo ${params}
        mkdir ./tmp
        export TMPDIR=./tmp
        ${simd}/bin/wasmd start --home . ${params} --rpc.laddr tcp://0.0.0.0:26657 --api.enable true --rpc.unsafe --api.address tcp://0.0.0.0:1317 --grpc.address 0.0.0.0:9090
      ''
    ];
    healthcheck = {
      interval = "5s";
      start_period = "20s";
      retries = 8;
      test = [
        "CMD-SHELL"
        ''
          curl http://127.0.0.1:26657/block?height=2 --fail || exit 1
        ''
      ];
    };
  };
}

