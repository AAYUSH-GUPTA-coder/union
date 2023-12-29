use std::{fmt::Display, marker::PhantomData};

use frame_support_procedural::{CloneNoBound, DebugNoBound, PartialEqNoBound};
use serde::{Deserialize, Serialize};
use unionlabs::{
    ibc::core::client::height::IsHeight,
    proof::ClientStatePath,
    traits::{ChainIdOf, ClientState, HeightOf},
};

use crate::{
    any_enum, defer, fetch,
    fetch::{AnyFetch, Fetch, FetchState},
    identified, now, seq, wait, AnyLightClientIdentified, ChainExt, DoFetchState, RelayerMsg,
};

any_enum! {
    /// Defines messages that are sent *to* the lightclient `L`.
    #[any = AnyWait]
    pub enum Wait<Hc: ChainExt, Tr: ChainExt> {
        Block(WaitForBlock<Hc, Tr>),
        Timestamp(WaitForTimestamp<Hc, Tr>),
        TrustedHeight(WaitForTrustedHeight<Hc, Tr>),
    }
}

impl<Hc, Tr> Wait<Hc, Tr>
where
    AnyLightClientIdentified<AnyWait>: From<identified!(Wait<Hc, Tr>)>,
    AnyLightClientIdentified<AnyFetch>: From<identified!(Fetch<Tr, Hc>)>,
    Hc: ChainExt + DoFetchState<Hc, Tr>,
    Tr: ChainExt,
{
    pub async fn handle(self, c: Hc) -> Vec<RelayerMsg> {
        match self {
            Wait::Block(WaitForBlock { height, __marker }) => {
                let chain_height = c.query_latest_height().await.unwrap();

                assert_eq!(
                    chain_height.revision_number(),
                    height.revision_number(),
                    "chain_height: {chain_height}, height: {height}",
                );

                if chain_height.revision_height() >= height.revision_height() {
                    [].into()
                } else {
                    [seq([
                        // REVIEW: Defer until `now + chain.block_time()`? Would require a new method on chain
                        defer(now() + 1),
                        wait::<Hc, Tr>(c.chain_id(), WaitForBlock { height, __marker }),
                    ])]
                    .into()
                }
            }
            Wait::Timestamp(WaitForTimestamp {
                timestamp,
                __marker,
            }) => {
                let chain_ts = c.query_latest_timestamp().await.unwrap();

                if chain_ts >= timestamp {
                    [].into()
                } else {
                    [seq([
                        // REVIEW: Defer until `now + chain.block_time()`? Would require a new method on chain
                        defer(now() + 1),
                        wait::<Hc, Tr>(
                            c.chain_id(),
                            WaitForTimestamp {
                                timestamp,
                                __marker,
                            },
                        ),
                    ])]
                    .into()
                }
            }
            Wait::TrustedHeight(WaitForTrustedHeight {
                client_id,
                counterparty_client_id,
                counterparty_chain_id,
                height,
            }) => {
                let latest_height = c.query_latest_height_as_destination().await.unwrap();

                let trusted_client_state =
                    Hc::query_client_state(&c, client_id.clone(), latest_height).await;

                if trusted_client_state.height().revision_height() >= height.revision_height() {
                    tracing::debug!(
                        "client height reached ({} >= {})",
                        trusted_client_state.height(),
                        height
                    );

                    [fetch::<Tr, Hc>(
                        counterparty_chain_id,
                        FetchState {
                            at: trusted_client_state.height(),
                            path: ClientStatePath {
                                client_id: counterparty_client_id.clone(),
                            }
                            .into(),
                        },
                    )]
                    .into()
                } else {
                    [seq([
                        // REVIEW: Defer until `now + counterparty_chain.block_time()`? Would require a new method on chain
                        defer(now() + 1),
                        wait::<Hc, Tr>(
                            c.chain_id(),
                            Wait::TrustedHeight(WaitForTrustedHeight {
                                client_id,
                                height,
                                counterparty_client_id,
                                counterparty_chain_id,
                            }),
                        ),
                    ])]
                    .into()
                }
            }
        }
    }
}

impl<Hc: ChainExt, Tr: ChainExt> Display for Wait<Hc, Tr> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Wait::Block(block) => write!(f, "Block({})", block.height),
            Wait::Timestamp(ts) => write!(f, "Timestamp({})", ts.timestamp),
            Wait::TrustedHeight(th) => write!(f, "TrustedHeight({})", th.height),
        }
    }
}

#[derive(DebugNoBound, CloneNoBound, PartialEqNoBound, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""), deny_unknown_fields)]
pub struct WaitForBlock<Hc: ChainExt, Tr: ChainExt> {
    pub height: HeightOf<Hc>,
    #[serde(skip)]
    pub __marker: PhantomData<fn() -> Tr>,
}

#[derive(DebugNoBound, CloneNoBound, PartialEqNoBound, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""), deny_unknown_fields)]
pub struct WaitForTimestamp<Hc: ChainExt, Tr: ChainExt> {
    pub timestamp: i64,
    #[serde(skip)]
    pub __marker: PhantomData<fn() -> (Hc, Tr)>,
}

#[derive(DebugNoBound, CloneNoBound, PartialEqNoBound, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""), deny_unknown_fields)]
pub struct WaitForTrustedHeight<Hc: ChainExt, Tr: ChainExt> {
    pub client_id: Hc::ClientId,
    pub counterparty_client_id: Tr::ClientId,
    pub counterparty_chain_id: ChainIdOf<Tr>,
    pub height: Tr::Height,
}
