import { getAddress } from "viem"
import { raise } from "$lib/utilities"
import type { Balance } from "../index.ts"
import { getPublicClient } from "@wagmi/core"
import { config } from "$lib/wallet/evm/config.ts"

export async function evmGasBalance({
  address,
  chainId
}: { address: string; chainId: string }): Promise<Balance> {
  const validAddress = getAddress(address)
  const chain = config.chains.find(chain => String(chain.id) === chainId)
  if (!chain) raise(`chain with id ${chainId} not found`)
  const viemClient = getPublicClient(config, {
    chainId: chain.id
  })
  const balance = await viemClient.getBalance({ address: validAddress })
  return { address: "", gasToken: true, ...chain.nativeCurrency, balance }
}

/**
 * for all chains predefined in `config` in `lib/wallet/evm/config.ts`
 */
export async function evmGasBalances({ address }: { address: string }): Promise<
  Array<{
    gasToken: boolean
    name: string
    symbol: string
    decimals: number
    balance: bigint
    chainId: string
  }>
> {
  const validAddress = getAddress(address)
  const viemClient = getPublicClient(config)

  return await Promise.all(
    config.chains.map(async chain => ({
      gasToken: true,
      ...chain.nativeCurrency,
      chainId: String(chain.id),
      balance: await viemClient.getBalance({ address: validAddress })
    }))
  )
}
