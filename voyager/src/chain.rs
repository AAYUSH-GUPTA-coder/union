use chain_utils::{
    cosmos::{Cosmos, CosmosInitError},
    evm::{Evm, EvmInitError},
    union::{Union, UnionInitError},
};
use unionlabs::ethereum::config::{Mainnet, Minimal};

use crate::{
    config::{ChainConfig, EvmChainConfig},
    queue::Queue,
};

pub enum AnyChain {
    Union(Union),
    Cosmos(Cosmos),
    EvmMainnet(Evm<Mainnet>),
    EvmMinimal(Evm<Minimal>),
}

#[derive(Debug, thiserror::Error)]
pub enum AnyChainTryFromConfigError {
    #[error("error initializing a union chain")]
    Union(#[from] UnionInitError),
    #[error("error initializing a cosmos chain")]
    Cosmos(#[from] CosmosInitError),
    #[error("error initializing an ethereum chain")]
    Evm(#[from] EvmInitError),
}

impl AnyChain {
    pub async fn try_from_config<Q: Queue>(
        config: ChainConfig,
    ) -> Result<Self, AnyChainTryFromConfigError> {
        Ok(match config {
            ChainConfig::Evm(EvmChainConfig::Mainnet(evm)) => Self::EvmMainnet(
                Evm::<Mainnet>::new(chain_utils::evm::Config {
                    ibc_handler_address: evm.ibc_handler_address,
                    signers: evm.signers,
                    eth_rpc_api: evm.eth_rpc_api,
                    eth_beacon_rpc_api: evm.eth_beacon_rpc_api,
                })
                .await?,
            ),
            ChainConfig::Evm(EvmChainConfig::Minimal(evm)) => Self::EvmMinimal(
                Evm::<Minimal>::new(chain_utils::evm::Config {
                    ibc_handler_address: evm.ibc_handler_address,
                    signers: evm.signers,
                    eth_rpc_api: evm.eth_rpc_api,
                    eth_beacon_rpc_api: evm.eth_beacon_rpc_api,
                })
                .await?,
            ),
            ChainConfig::Union(union) => Self::Union(
                Union::new(chain_utils::union::Config {
                    signers: union.signers,
                    ws_url: union.ws_url,
                    prover_endpoint: union.prover_endpoint,
                    grpc_url: union.grpc_url,
                    fee_denom: union.fee_denom,
                })
                .await?,
            ),
            ChainConfig::Cosmos(cosmos) => Self::Cosmos(
                Cosmos::new(chain_utils::cosmos::Config {
                    signers: cosmos.signers,
                    ws_url: cosmos.ws_url,
                    grpc_url: cosmos.grpc_url,
                    fee_denom: cosmos.fee_denom,
                })
                .await?,
            ),
        })
    }
}
