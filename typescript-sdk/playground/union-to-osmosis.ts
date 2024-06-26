#!/usr/bin/env bun
import "#patch.ts"
import { parseArgs } from "node:util"
import { cosmosHttp } from "#transport.ts"
import { raise } from "#utilities/index.ts"
import { hexStringToUint8Array } from "#convert.ts"
import { consola, timestamp } from "../scripts/logger.ts"
import { DirectSecp256k1Wallet } from "@cosmjs/proto-signing"
import { createCosmosSdkClient, offchainQuery, type TransferAssetsParameters } from "#mod.ts"

/**
 *
 *
 * W  I  P
 *
 *
 */

/* `bun playground/union-to-osmosis.ts --private-key "..."` */

const { values } = parseArgs({
  args: process.argv.slice(2),
  options: {
    "private-key": { type: "string" },
    "estimate-gas": { type: "boolean", default: false }
  }
})

const PRIVATE_KEY = values["private-key"]
if (!PRIVATE_KEY) throw new Error("Private key not found")
const ONLY_ESTIMATE_GAS = values["estimate-gas"] ?? false

const cosmosAccount = await DirectSecp256k1Wallet.fromKey(
  Uint8Array.from(hexStringToUint8Array(PRIVATE_KEY)),
  "union"
)

const stamp = timestamp()

// const relayContractAddress = "union1eumfw2ppz8cwl8xdh3upttzp5rdyms48kqhm30f8g9u4zwj0pprqg2vmu3"

try {
  const {
    data: [unionTestnetInfo]
  } = await offchainQuery.chain({
    includeContracts: true,
    chainId: "union-testnet-8"
  })

  if (!unionTestnetInfo) raise("Union testnet info not found")

  const ucsConfiguration = unionTestnetInfo.ucs1_configurations
    ?.filter(config => config.destination_chain.chain_id === "osmo-test-5")
    .at(0)

  if (!ucsConfiguration) raise("UCS configuration not found")

  const client = createCosmosSdkClient({
    // @ts-expect-error
    evm: {},
    cosmos: {
      account: cosmosAccount,
      gasPrice: { amount: "0.0025", denom: "muno" },
      transport: cosmosHttp("https://rpc.testnet.bonlulu.uno")
    }
  })

  const transactionObject = {
    amount: 1n,
    network: "cosmos",
    denomAddress: "muno",
    sourceChannel: ucsConfiguration.channel_id,
    relayContractAddress: ucsConfiguration.contract_address,
    recipient: "osmo14qemq0vw6y3gc3u3e0aty2e764u4gs5l32ydm0",
    path: [ucsConfiguration.source_chain.chain_id, ucsConfiguration.destination_chain.chain_id]
  } satisfies TransferAssetsParameters

  console.info(transactionObject)

  const gasCostResponse = await client.simulateTransaction(transactionObject)

  console.info(`Gas cost: ${gasCostResponse.data}`)

  if (ONLY_ESTIMATE_GAS) process.exit(0)

  const hash = await client.transferAsset(transactionObject)
  consola.info(hash)
} catch (error) {
  const errorMessage = error instanceof Error ? error.message : error
  console.error(errorMessage)
} finally {
  process.exit(0)
}
