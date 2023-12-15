import {
  apolloClient,
  tendermintClient,
  stargateClient,
  unionAccount,
  unionUnoBalance,
  ethersProvider,
  ethersSigner,
  ethereumAddress,
  cosmjsSigner,
  cosmwasmClient,
  ethereumUnoBalance,
  ethereumEthBalance,
} from "$lib/stores/wallets";
import { ApolloClient, InMemoryCache, gql } from "@apollo/client/core";
import { get } from "svelte/store";
import { ethers } from "ethers";
import { GasPrice } from "@cosmjs/stargate";
import { SigningCosmWasmClient } from "@cosmjs/cosmwasm-stargate";

import { UCS01_UNION_SOURCE_CHANNEL, UNION_RPC_URL } from "./constants";

import {
  UNION_CHAIN_ID,
  ERC20_CONTRACT_ABI,
  UCS01_UNION_ADDRESS,
  MUNO_ERC20_ADDRESS,
} from "./constants";

export const initClients = async (): Promise<void> => {
  // Hack to import cosmjs
  // @ts-ignore
  window.process = { env: {} };

  const { CosmjsOfflineSigner } = await import(
    "@leapwallet/cosmos-snap-provider"
  );
  const { GasPrice, SigningStargateClient } = await import("@cosmjs/stargate");
  const { Tendermint37Client } = await import("@cosmjs/tendermint-rpc");
  const offlineSigner = new CosmjsOfflineSigner(UNION_CHAIN_ID);
  cosmjsSigner.set(offlineSigner);

  let accounts = await offlineSigner.getAccounts();
  if (accounts.length > 0) {
    unionAccount.set(accounts[0]);
  }

  tendermintClient.set(await Tendermint37Client.connect(UNION_RPC_URL));
  let tmClient = get(tendermintClient);
  if (tmClient == null) {
    return;
  }
  stargateClient.set(
    await SigningStargateClient.createWithSigner(tmClient, offlineSigner, {
      gasPrice: GasPrice.fromString("0.001muno"),
    })
  );

  apolloClient.set(
    new ApolloClient({
      uri: "https://graphql.union.build/v1/graphql",
      cache: new InMemoryCache(),
    })
  );
  initCosmwasmClient();
};

const GET_UNO_FROM_FAUCET = gql`
  mutation MyMutation($addr: Address!) {
    union {
      send(input: { toAddress: $addr })
    }
  }
`;

export const getUnoFromFaucet = async () => {
  const uAccount = get(unionAccount);
  const apollo = get(apolloClient);
  if (uAccount === null || apollo === null) {
    console.error(
      "trying to get uno from faucet before accounts are loaded or apollo client has not been init"
    );
    return;
  }

  let _response = await apollo.mutate({
    mutation: GET_UNO_FROM_FAUCET,
    variables: { addr: uAccount.address },
  });
};

export const sendUnoToUnionAddress = async () => {
  const sgClient = get(stargateClient);
  const uAccount = get(unionAccount);
  if (sgClient === null || uAccount === null) {
    console.error("trying to get uno from faucet before accounts are loaded");
    return;
  }
  const _txResponse = await sgClient.sendTokens(
    uAccount.address,
    "union1v39zvpn9ff7quu9lxsawdwpg60lyfpz8pmhfey",
    [{ denom: "muno", amount: "1000" }],
    "auto"
  );
};

export const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

const balanceWorker = async (
  fetcher: () => Promise<void>,
  interval: number
) => {
  for (; ;) {
    fetcher();
    await sleep(interval);
  }
};

export const startBalanceWorkers = () => {
  balanceWorker(updateEthereumUnoBalance, 2500);
  balanceWorker(updateEthereumEthBalance, 2500);
  balanceWorker(updateUnionUnoBalance, 2500);
};

export const updateUnionUnoBalance = async () => {
  const sgClient = get(stargateClient);
  const uAccount = get(unionAccount);
  if (sgClient == null) {
    console.error("stargateClient is null while querying balance");
    return;
  }
  if (uAccount == null) {
    console.error("fetching balance for nonexisting account");
    return;
  }
  unionUnoBalance.set(await sgClient.getBalance(uAccount.address, "muno"));
};
export const updateEthereumEthBalance = async () => {
  const eProvider = get(ethersProvider);
  const address = get(ethereumAddress);
  if (eProvider === null) {
    console.error("ethereum provider is null when fetching balance");
    return;
  }
  if (address === null) {
    console.error("trying to fetch ethereum balance, but address is null");
    return;
  }
  const balance = await eProvider.getBalance(address);
  ethereumEthBalance.set(balance);
};

export const initCosmwasmClient = async () => {
  const tmClient = get(tendermintClient);
  const cSigner = get(cosmjsSigner);
  if (tmClient === null || cSigner === null) {
    console.error("need tm client and cosmjs signer to init cosmwasmclient");
    return;
  }

  const cwClient = await SigningCosmWasmClient.createWithSigner(
    tmClient,
    cSigner,
    {
      gasPrice: GasPrice.fromString("0.001muno"),
    }
  );
  cosmwasmClient.set(cwClient);
};

export const sendUnoToEthereum = async () => {
  const cwClient = get(cosmwasmClient);
  const uAccount = get(unionAccount);
  const eAddress = get(ethereumAddress);
  console.log("after execute");

  if (cwClient === null || uAccount === null || eAddress === null) {
    console.error("please init dependencies for uno transfers");
    return;
  }

  console.log("before execute");
  console.log("eAddress", eAddress);

  return cwClient.execute(
    uAccount.address,
    UCS01_UNION_ADDRESS,
    {
      transfer: {
        channel: UCS01_UNION_SOURCE_CHANNEL,
        receiver: eAddress.substr(2),
        timeout: null,
        memo: "random more than four characters I'm transferring.",
      },
    },
    "auto",
    undefined,
    [{ denom: "muno", amount: "1000" }]
  );
};

export const updateEthereumUnoBalance = async () => {
  const eProvider = get(ethersProvider);
  const eAddress = get(ethereumAddress);
  const eSigner = get(ethersSigner);
  const uAccount = get(unionAccount);

  if (
    eProvider === null ||
    eAddress === null ||
    eSigner === null ||
    uAccount === null
  ) {
    console.error("missing dependencies for updateEthereumUnoBalance ");
    return;
  }

  const contract = new ethers.Contract(
    MUNO_ERC20_ADDRESS,
    ERC20_CONTRACT_ABI,
    eProvider
  );
  const balance = await contract.balanceOf(eAddress);
  ethereumUnoBalance.set(balance);
};
