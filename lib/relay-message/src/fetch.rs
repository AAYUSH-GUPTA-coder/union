use std::{
    fmt::{Debug, Display},
    marker::PhantomData,
    num::NonZeroU64,
};

use chain_utils::GetChain;
use futures::Future;
use macros::apply;
use queue_msg::{data, fetch, msg_struct, HandleFetch, QueueError, QueueMsg, QueueMsgTypes};
use unionlabs::{
    hash::H256,
    id::{ChannelId, PortId},
    proof::{self, ClientStatePath},
    traits::{ChainIdOf, ClientIdOf, HeightOf},
    QueryHeight,
};

use crate::{
    any_enum, any_lc,
    data::{
        AnyData, Data, LatestHeight, PacketAcknowledgement, SelfClientState, SelfConsensusState,
    },
    id, identified, AnyLightClientIdentified, ChainExt, DoFetchProof, DoFetchState,
    DoFetchUpdateHeaders, RelayerMsgTypes,
};

#[apply(any_enum)]
/// Fetch some data that will likely be used in a [`QueueMsg::Aggregate`].
#[any = AnyFetch]
#[specific = LightClientSpecificFetch]
pub enum Fetch<Hc: ChainExt, Tr: ChainExt> {
    State(FetchState<Hc, Tr>),
    Proof(FetchProof<Hc, Tr>),

    LatestHeight(FetchLatestHeight<Hc, Tr>),

    LatestClientState(FetchLatestClientState<Hc, Tr>),

    SelfClientState(FetchSelfClientState<Hc, Tr>),
    SelfConsensusState(FetchSelfConsensusState<Hc, Tr>),

    PacketAcknowledgement(FetchPacketAcknowledgement<Hc, Tr>),

    UpdateHeaders(FetchUpdateHeaders<Hc, Tr>),

    #[serde(untagged)]
    LightClientSpecific(LightClientSpecificFetch<Hc, Tr>),
}

impl HandleFetch<RelayerMsgTypes> for AnyLightClientIdentified<AnyFetch> {
    async fn handle(
        self,
        store: &<RelayerMsgTypes as QueueMsgTypes>::Store,
    ) -> Result<QueueMsg<RelayerMsgTypes>, QueueError> {
        let fetch = self;

        any_lc! {
            |fetch| {
                Ok(store
                    .with_chain(&fetch.chain_id, move |c| async move { fetch.t.handle(c).await })
                    .map_err(|e| QueueError::Fatal(Box::new(e)))?
                    .await)
            }
        }
    }
}

pub trait DoFetch<Hc: ChainExt, Tr: ChainExt>: Sized + Debug + Clone + PartialEq {
    fn do_fetch(c: &Hc, _: Self) -> impl Future<Output = QueueMsg<RelayerMsgTypes>>;
}

impl<Hc: ChainExt, Tr: ChainExt> Display for Fetch<Hc, Tr> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let path_display = |path: &_| match path {
            proof::Path::ClientStatePath(_) => "ClientState",
            proof::Path::ClientConsensusStatePath(_) => "ClientConsensusState",
            proof::Path::ConnectionPath(_) => "Connection",
            proof::Path::ChannelEndPath(_) => "ChannelEnd",
            proof::Path::CommitmentPath(_) => "Commitment",
            proof::Path::AcknowledgementPath(_) => "Acknowledgement",
        };

        match self {
            Fetch::State(fetch) => write!(f, "State({})", path_display(&fetch.path)),
            Fetch::Proof(fetch) => write!(f, "Proof({})", path_display(&fetch.path)),
            Fetch::LatestHeight(_) => write!(f, "LatestHeight"),
            Fetch::LatestClientState(_) => write!(f, "LatestClientState"),
            Fetch::SelfClientState(_) => write!(f, "SelfClientState"),
            Fetch::SelfConsensusState(_) => write!(f, "SelfConsensusState"),
            Fetch::PacketAcknowledgement(_) => write!(f, "PacketAcknowledgement"),
            Fetch::UpdateHeaders(_) => write!(f, "UpdateHeaders"),
            Fetch::LightClientSpecific(fetch) => write!(f, "LightClientSpecific({})", fetch.0),
        }
    }
}

#[apply(msg_struct)]
#[cover(Tr)]
pub struct FetchSelfClientState<Hc: ChainExt, Tr: ChainExt> {
    pub at: QueryHeight<HeightOf<Hc>>,
}

#[apply(msg_struct)]
#[cover(Tr)]
pub struct FetchSelfConsensusState<Hc: ChainExt, Tr: ChainExt> {
    pub at: QueryHeight<HeightOf<Hc>>,
}
#[apply(msg_struct)]
pub struct FetchProof<Hc: ChainExt, Tr: ChainExt> {
    pub at: HeightOf<Hc>,
    pub path: proof::Path<Hc::ClientId, Tr::Height>,
}

#[apply(msg_struct)]
pub struct FetchState<Hc: ChainExt, Tr: ChainExt> {
    pub at: HeightOf<Hc>,
    pub path: proof::Path<Hc::ClientId, Tr::Height>,
}

#[apply(msg_struct)]
#[cover(Tr)]
pub struct FetchLatestClientState<Hc: ChainExt, Tr: ChainExt> {
    pub path: ClientStatePath<Hc::ClientId>,
}

#[apply(msg_struct)]
#[cover(Hc, Tr)]
pub struct FetchPacketAcknowledgement<Hc: ChainExt, Tr: ChainExt> {
    pub tx_hash: H256,
    pub destination_port_id: PortId,
    pub destination_channel_id: ChannelId,
    pub sequence: NonZeroU64,
}

#[apply(msg_struct)]
pub struct FetchUpdateHeaders<Hc: ChainExt, Tr: ChainExt> {
    pub counterparty_chain_id: ChainIdOf<Tr>,
    // id of the counterparty client that will be updated with the fetched headers
    pub counterparty_client_id: ClientIdOf<Tr>,
    pub update_from: HeightOf<Hc>,
    pub update_to: HeightOf<Hc>,
}

#[apply(msg_struct)]
#[cover(Hc, Tr)]
pub struct FetchLatestHeight<Hc: ChainExt, Tr: ChainExt> {}

#[apply(msg_struct)]
pub struct LightClientSpecificFetch<Hc: ChainExt, Tr: ChainExt>(pub Hc::Fetch<Tr>);

impl<Hc, Tr> Fetch<Hc, Tr>
where
    Hc: ChainExt + DoFetchState<Hc, Tr> + DoFetchProof<Hc, Tr> + DoFetchUpdateHeaders<Hc, Tr>,
    Hc::Fetch<Tr>: DoFetch<Hc, Tr>,
    Tr: ChainExt,
    AnyLightClientIdentified<AnyData>: From<identified!(Data<Hc, Tr>)>,
    AnyLightClientIdentified<AnyFetch>: From<identified!(Fetch<Hc, Tr>)>,
{
    pub async fn handle(self, c: Hc) -> QueueMsg<RelayerMsgTypes> {
        match self {
            Fetch::Proof(msg) => Hc::proof(&c, msg.at, msg.path),
            Fetch::State(msg) => Hc::state(&c, msg.at, msg.path),
            Fetch::LatestHeight(FetchLatestHeight { __marker: _ }) => data(id(
                c.chain_id(),
                LatestHeight {
                    height: c.query_latest_height().await.unwrap(),
                    __marker: PhantomData,
                },
            )),
            Fetch::SelfClientState(FetchSelfClientState {
                at: height,
                __marker: _,
            }) => {
                // TODO: Split this into a separate query and aggregate
                let height = match height {
                    QueryHeight::Latest => c.query_latest_height().await.unwrap(),
                    QueryHeight::Specific(h) => h,
                };

                data(id::<Hc, Tr, _>(
                    c.chain_id(),
                    SelfClientState {
                        self_client_state: c.self_client_state(height).await,
                        __marker: PhantomData,
                    },
                ))
            }
            Fetch::SelfConsensusState(FetchSelfConsensusState {
                at: height,
                __marker: _,
            }) => {
                // TODO: Split this into a separate query and aggregate
                let height = match height {
                    QueryHeight::Latest => c.query_latest_height().await.unwrap(),
                    QueryHeight::Specific(h) => h,
                };

                data(id::<Hc, Tr, _>(
                    c.chain_id(),
                    SelfConsensusState {
                        self_consensus_state: c.self_consensus_state(height).await,
                        __marker: PhantomData,
                    },
                ))
            }
            Fetch::PacketAcknowledgement(FetchPacketAcknowledgement {
                tx_hash,
                destination_port_id,
                destination_channel_id,
                sequence,
                __marker,
            }) => {
                let ack = c
                    .read_ack(
                        tx_hash.clone(),
                        destination_channel_id.clone(),
                        destination_port_id.clone(),
                        sequence,
                    )
                    .await;

                data(id::<Hc, Tr, _>(
                    c.chain_id(),
                    PacketAcknowledgement {
                        fetched_by: FetchPacketAcknowledgement {
                            tx_hash,
                            destination_port_id,
                            destination_channel_id,
                            sequence,
                            __marker,
                        },
                        ack,
                    },
                ))
            }
            Fetch::UpdateHeaders(fetch_update_headers) => {
                Hc::fetch_update_headers(&c, fetch_update_headers)
            }
            Fetch::LightClientSpecific(LightClientSpecificFetch(fetch)) => c.do_fetch(fetch).await,
            Fetch::LatestClientState(FetchLatestClientState { path, __marker }) => {
                fetch(id::<Hc, Tr, _>(
                    c.chain_id(),
                    FetchState {
                        at: c.query_latest_height().await.unwrap(),
                        path: path.into(),
                    },
                ))
            }
        }
    }
}
