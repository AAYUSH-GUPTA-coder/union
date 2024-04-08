#![feature(min_exhaustive_patterns)]
#![allow(clippy::large_enum_variant)]

use std::{collections::VecDeque, fmt::Debug, str::FromStr};

use block_message::BlockMessageTypes;
use chain_utils::{
    cosmos::Cosmos, ethereum::Ethereum, scroll::Scroll, union::Union, wasm::Wasm, Chains,
};
use queue_msg::{
    event, queue_msg, HandleAggregate, HandleData, HandleEffect, HandleEvent, HandleFetch,
    HandleWait, QueueError, QueueMsg, QueueMsgTypes,
};
use relay_message::RelayMessageTypes;
use unionlabs::{
    ethereum::config::{Mainnet, Minimal},
    events::{
        AcknowledgePacket, ChannelOpenAck, ChannelOpenConfirm, ChannelOpenInit, ChannelOpenTry,
        ClientMisbehaviour, ConnectionOpenAck, ConnectionOpenConfirm, ConnectionOpenInit,
        ConnectionOpenTry, CreateClient, IbcEvent, RecvPacket, SendPacket, SubmitEvidence,
        TimeoutPacket, UpdateClient, WriteAcknowledgement,
    },
    traits::ClientIdOf,
    WasmClientType,
};

pub struct VoyagerMessageTypes;

impl QueueMsgTypes for VoyagerMessageTypes {
    type Event = VoyagerEvent;
    type Data = VoyagerData;
    type Fetch = VoyagerFetch;
    type Effect = VoyagerMsg;
    type Wait = VoyagerWait;
    type Aggregate = VoyagerAggregate;

    type Store = Chains;
}

pub trait FromQueueMsg<T: QueueMsgTypes>: QueueMsgTypes + Sized {
    fn from_queue_msg(value: QueueMsg<T>) -> QueueMsg<Self>;
}

impl FromQueueMsg<RelayMessageTypes> for VoyagerMessageTypes {
    fn from_queue_msg(value: QueueMsg<RelayMessageTypes>) -> QueueMsg<Self> {
        match value {
            QueueMsg::Event(event) => QueueMsg::Event(VoyagerEvent::Relay(event)),
            QueueMsg::Data(data) => QueueMsg::Data(VoyagerData::Relay(data)),
            QueueMsg::Fetch(fetch) => QueueMsg::Fetch(VoyagerFetch::Relay(fetch)),
            QueueMsg::Effect(msg) => QueueMsg::Effect(VoyagerMsg::Relay(msg)),
            QueueMsg::Wait(wait) => QueueMsg::Wait(VoyagerWait::Relay(wait)),
            QueueMsg::Defer(defer) => QueueMsg::Defer(defer),
            QueueMsg::Repeat { times, msg } => QueueMsg::Repeat {
                times,
                msg: Box::new(Self::from_queue_msg(*msg)),
            },
            QueueMsg::Timeout {
                timeout_timestamp,
                msg,
            } => QueueMsg::Timeout {
                timeout_timestamp,
                msg: Box::new(Self::from_queue_msg(*msg)),
            },
            QueueMsg::Sequence(seq) => {
                QueueMsg::Sequence(seq.into_iter().map(Self::from_queue_msg).collect())
            }
            QueueMsg::Concurrent(seq) => {
                QueueMsg::Concurrent(seq.into_iter().map(Self::from_queue_msg).collect())
            }
            QueueMsg::Retry { remaining, msg } => QueueMsg::Retry {
                remaining,
                msg: Box::new(Self::from_queue_msg(*msg)),
            },
            QueueMsg::Aggregate {
                queue,
                data,
                receiver,
            } => QueueMsg::Aggregate {
                queue: queue.into_iter().map(Self::from_queue_msg).collect(),
                data: data.into_iter().map(VoyagerData::Relay).collect(),
                receiver: VoyagerAggregate::Relay(receiver),
            },
            QueueMsg::Noop => QueueMsg::Noop,
        }
    }
}

impl FromQueueMsg<BlockMessageTypes> for VoyagerMessageTypes {
    fn from_queue_msg(value: QueueMsg<BlockMessageTypes>) -> QueueMsg<Self> {
        match value {
            QueueMsg::Data(data) => QueueMsg::Data(VoyagerData::Block(data)),
            QueueMsg::Fetch(fetch) => QueueMsg::Fetch(VoyagerFetch::Block(fetch)),
            QueueMsg::Wait(wait) => QueueMsg::Wait(VoyagerWait::Block(wait)),
            QueueMsg::Defer(defer) => QueueMsg::Defer(defer),
            QueueMsg::Repeat { times, msg } => QueueMsg::Repeat {
                times,
                msg: Box::new(Self::from_queue_msg(*msg)),
            },
            QueueMsg::Timeout {
                timeout_timestamp,
                msg,
            } => QueueMsg::Timeout {
                timeout_timestamp,
                msg: Box::new(Self::from_queue_msg(*msg)),
            },
            QueueMsg::Sequence(seq) => {
                QueueMsg::Sequence(seq.into_iter().map(Self::from_queue_msg).collect())
            }
            QueueMsg::Concurrent(seq) => {
                QueueMsg::Concurrent(seq.into_iter().map(Self::from_queue_msg).collect())
            }
            QueueMsg::Retry { remaining, msg } => QueueMsg::Retry {
                remaining,
                msg: Box::new(Self::from_queue_msg(*msg)),
            },
            QueueMsg::Aggregate {
                queue,
                data,
                receiver,
            } => QueueMsg::Aggregate {
                queue: queue.into_iter().map(Self::from_queue_msg).collect(),
                data: data.into_iter().map(VoyagerData::Block).collect(),
                receiver: VoyagerAggregate::Block(receiver),
            },
            QueueMsg::Noop => QueueMsg::Noop,
        }
    }
}

#[queue_msg]
pub enum VoyagerMsg {
    Block(<BlockMessageTypes as QueueMsgTypes>::Effect),
    Relay(<RelayMessageTypes as QueueMsgTypes>::Effect),
}

impl HandleEffect<VoyagerMessageTypes> for VoyagerMsg {
    async fn handle(
        self,
        store: &<VoyagerMessageTypes as QueueMsgTypes>::Store,
    ) -> Result<QueueMsg<VoyagerMessageTypes>, QueueError> {
        Ok(match self {
            Self::Relay(msg) => {
                <VoyagerMessageTypes as FromQueueMsg<RelayMessageTypes>>::from_queue_msg(
                    msg.handle(store).await?,
                )
            }
        })
    }
}

#[queue_msg]
pub enum VoyagerWait {
    Block(<BlockMessageTypes as QueueMsgTypes>::Wait),
    Relay(<RelayMessageTypes as QueueMsgTypes>::Wait),
}

impl HandleWait<VoyagerMessageTypes> for VoyagerWait {
    async fn handle(
        self,
        store: &<VoyagerMessageTypes as QueueMsgTypes>::Store,
    ) -> Result<QueueMsg<VoyagerMessageTypes>, QueueError> {
        Ok(match self {
            Self::Block(msg) => {
                <VoyagerMessageTypes as FromQueueMsg<BlockMessageTypes>>::from_queue_msg(
                    HandleWait::<BlockMessageTypes>::handle(msg, store).await?,
                )
            }
            Self::Relay(msg) => {
                <VoyagerMessageTypes as FromQueueMsg<RelayMessageTypes>>::from_queue_msg(
                    HandleWait::<RelayMessageTypes>::handle(msg, store).await?,
                )
            }
        })
    }
}

#[queue_msg]
pub enum VoyagerAggregate {
    Block(<BlockMessageTypes as QueueMsgTypes>::Aggregate),
    Relay(<RelayMessageTypes as QueueMsgTypes>::Aggregate),
}

impl HandleAggregate<VoyagerMessageTypes> for VoyagerAggregate {
    fn handle(
        self,
        data: VecDeque<<VoyagerMessageTypes as QueueMsgTypes>::Data>,
    ) -> Result<QueueMsg<VoyagerMessageTypes>, QueueError> {
        Ok(match self {
            Self::Block(aggregate) => VoyagerMessageTypes::from_queue_msg(
                aggregate.handle(
                    data.into_iter()
                        .map(|d| match d {
                            VoyagerData::Block(d) => d,
                            VoyagerData::Relay(_) => panic!(),
                        })
                        .collect(),
                )?,
            ),
            Self::Relay(aggregate) => VoyagerMessageTypes::from_queue_msg(
                aggregate.handle(
                    data.into_iter()
                        .map(|d| match d {
                            VoyagerData::Block(_) => panic!(),
                            VoyagerData::Relay(d) => d,
                        })
                        .collect(),
                )?,
            ),
        })
    }
}

#[queue_msg]
pub enum VoyagerEvent {
    Block(<BlockMessageTypes as QueueMsgTypes>::Event),
    Relay(<RelayMessageTypes as QueueMsgTypes>::Event),
}

impl HandleEvent<VoyagerMessageTypes> for VoyagerEvent {
    fn handle(
        self,
        store: &<VoyagerMessageTypes as QueueMsgTypes>::Store,
    ) -> Result<QueueMsg<VoyagerMessageTypes>, QueueError> {
        Ok(match self {
            Self::Relay(event) => {
                <VoyagerMessageTypes as FromQueueMsg<RelayMessageTypes>>::from_queue_msg(
                    HandleEvent::handle(event, store)?,
                )
            }
        })
    }
}

#[queue_msg]
pub enum VoyagerData {
    Block(<BlockMessageTypes as QueueMsgTypes>::Data),
    Relay(<RelayMessageTypes as QueueMsgTypes>::Data),
}

impl HandleData<VoyagerMessageTypes> for VoyagerData {
    fn handle(
        self,
        store: &<VoyagerMessageTypes as QueueMsgTypes>::Store,
    ) -> Result<QueueMsg<VoyagerMessageTypes>, QueueError> {
        Ok(match self {
            Self::Block(data) => match data.handle(store)? {
                QueueMsg::Data(block_message::AnyChainIdentified::Cosmos(
                    block_message::Identified {
                        chain_id,
                        t: block_message::data::Data::IbcEvent(ibc_event),
                    },
                )) => <VoyagerMessageTypes as FromQueueMsg<RelayMessageTypes>>::from_queue_msg(
                    match ibc_event.client_type {
                        unionlabs::ClientType::Wasm(unionlabs::WasmClientType::Cometbls) => {
                            event::<RelayMessageTypes>(relay_message::id::<
                                Wasm<Cosmos>,
                                Wasm<Union>,
                                _,
                            >(
                                chain_id,
                                relay_message::event::IbcEvent {
                                    tx_hash: ibc_event.tx_hash,
                                    height: ibc_event.height,
                                    event: chain_event_to_lc_event::<Wasm<Cosmos>, Wasm<Union>>(
                                        ibc_event.event,
                                    ),
                                },
                            ))
                        }
                        unionlabs::ClientType::Tendermint => {
                            event::<RelayMessageTypes>(relay_message::id::<Cosmos, Cosmos, _>(
                                chain_id,
                                relay_message::event::IbcEvent {
                                    tx_hash: ibc_event.tx_hash,
                                    height: ibc_event.height,
                                    event: chain_event_to_lc_event::<Cosmos, Cosmos>(
                                        ibc_event.event,
                                    ),
                                },
                            ))
                        }
                        _ => unimplemented!(),
                    },
                ),
                QueueMsg::Data(block_message::AnyChainIdentified::Union(
                    block_message::Identified {
                        chain_id,
                        t: block_message::data::Data::IbcEvent(ibc_event),
                    },
                )) => <VoyagerMessageTypes as FromQueueMsg<RelayMessageTypes>>::from_queue_msg(
                    match ibc_event.client_type {
                        unionlabs::ClientType::Wasm(unionlabs::WasmClientType::EthereumMinimal) => {
                            event(relay_message::id::<Wasm<Union>, Ethereum<Minimal>, _>(
                                chain_id,
                                relay_message::event::IbcEvent {
                                    tx_hash: ibc_event.tx_hash,
                                    height: ibc_event.height,
                                    event: chain_event_to_lc_event::<Union, Ethereum<Minimal>>(
                                        ibc_event.event,
                                    ),
                                },
                            ))
                        }
                        unionlabs::ClientType::Wasm(unionlabs::WasmClientType::EthereumMainnet) => {
                            event(relay_message::id::<Wasm<Union>, Ethereum<Mainnet>, _>(
                                chain_id,
                                relay_message::event::IbcEvent {
                                    tx_hash: ibc_event.tx_hash,
                                    height: ibc_event.height,
                                    event: chain_event_to_lc_event::<Union, Ethereum<Mainnet>>(
                                        ibc_event.event,
                                    ),
                                },
                            ))
                        }
                        unionlabs::ClientType::Wasm(unionlabs::WasmClientType::Scroll) => {
                            event(relay_message::id::<Wasm<Union>, Scroll, _>(
                                chain_id,
                                relay_message::event::IbcEvent {
                                    tx_hash: ibc_event.tx_hash,
                                    height: ibc_event.height,
                                    event: chain_event_to_lc_event::<Union, Scroll>(
                                        ibc_event.event,
                                    ),
                                },
                            ))
                        }
                        unionlabs::ClientType::Wasm(unionlabs::WasmClientType::Tendermint) => {
                            event(relay_message::id::<Wasm<Union>, Wasm<Cosmos>, _>(
                                chain_id,
                                relay_message::event::IbcEvent {
                                    tx_hash: ibc_event.tx_hash,
                                    height: ibc_event.height,
                                    event: chain_event_to_lc_event::<Wasm<Union>, Wasm<Cosmos>>(
                                        ibc_event.event,
                                    ),
                                },
                            ))
                        }
                        _ => unimplemented!(),
                    },
                ),
                QueueMsg::Data(block_message::AnyChainIdentified::EthMainnet(
                    block_message::Identified {
                        chain_id,
                        t: block_message::data::Data::IbcEvent(ibc_event),
                    },
                )) => <VoyagerMessageTypes as FromQueueMsg<RelayMessageTypes>>::from_queue_msg(
                    match ibc_event.client_type {
                        unionlabs::ClientType::Cometbls => {
                            event(relay_message::id::<Ethereum<Mainnet>, Wasm<Union>, _>(
                                chain_id,
                                relay_message::event::IbcEvent {
                                    tx_hash: ibc_event.tx_hash,
                                    height: ibc_event.height,
                                    event: chain_event_to_lc_event::<Ethereum<Mainnet>, Wasm<Union>>(
                                        ibc_event.event,
                                    ),
                                },
                            ))
                        }
                        _ => unimplemented!(),
                    },
                ),
                QueueMsg::Data(block_message::AnyChainIdentified::EthMinimal(
                    block_message::Identified {
                        chain_id,
                        t: block_message::data::Data::IbcEvent(ibc_event),
                    },
                )) => <VoyagerMessageTypes as FromQueueMsg<RelayMessageTypes>>::from_queue_msg(
                    match ibc_event.client_type {
                        unionlabs::ClientType::Cometbls => {
                            event(relay_message::id::<Ethereum<Minimal>, Wasm<Union>, _>(
                                chain_id,
                                relay_message::event::IbcEvent {
                                    tx_hash: ibc_event.tx_hash,
                                    height: ibc_event.height,
                                    event: chain_event_to_lc_event::<Ethereum<Minimal>, Wasm<Union>>(
                                        ibc_event.event,
                                    ),
                                },
                            ))
                        }
                        _ => unimplemented!(),
                    },
                ),
                msg => {
                    <VoyagerMessageTypes as FromQueueMsg<BlockMessageTypes>>::from_queue_msg(msg)
                }
            },
            Self::Relay(data) => {
                <VoyagerMessageTypes as FromQueueMsg<RelayMessageTypes>>::from_queue_msg(
                    data.handle(store)?,
                )
            }
        })
    }
}

#[queue_msg]
pub enum VoyagerFetch {
    Block(<BlockMessageTypes as QueueMsgTypes>::Fetch),
    Relay(<RelayMessageTypes as QueueMsgTypes>::Fetch),
}

impl HandleFetch<VoyagerMessageTypes> for VoyagerFetch {
    async fn handle(
        self,
        store: &<VoyagerMessageTypes as QueueMsgTypes>::Store,
    ) -> Result<QueueMsg<VoyagerMessageTypes>, QueueError> {
        Ok(match self {
            Self::Block(fetch) => {
                <VoyagerMessageTypes as FromQueueMsg<BlockMessageTypes>>::from_queue_msg(
                    fetch.handle(store).await?,
                )
            }
            Self::Relay(fetch) => {
                <VoyagerMessageTypes as FromQueueMsg<RelayMessageTypes>>::from_queue_msg(
                    fetch.handle(store).await?,
                )
            }
        })
    }
}

// poor man's monad
fn chain_event_to_lc_event<Hc: relay_message::ChainExt, Tr: relay_message::ChainExt>(
    event: IbcEvent<Hc::ClientId, Hc::ClientType, String>,
) -> IbcEvent<Hc::ClientId, Hc::ClientType, Tr::ClientId>
where
    <ClientIdOf<Tr> as FromStr>::Err: Debug,
{
    match event {
        IbcEvent::CreateClient(CreateClient {
            client_id,
            client_type,
            consensus_height,
        }) => IbcEvent::CreateClient(CreateClient {
            client_id,
            client_type,
            consensus_height,
        }),
        IbcEvent::UpdateClient(UpdateClient {
            client_id,
            client_type,
            consensus_heights,
        }) => IbcEvent::UpdateClient(UpdateClient {
            client_id,
            client_type,
            consensus_heights,
        }),
        IbcEvent::ClientMisbehaviour(ClientMisbehaviour {
            client_id,
            client_type,
            consensus_height,
        }) => IbcEvent::ClientMisbehaviour(ClientMisbehaviour {
            client_id,
            client_type,
            consensus_height,
        }),
        IbcEvent::SubmitEvidence(SubmitEvidence { evidence_hash }) => {
            IbcEvent::SubmitEvidence(SubmitEvidence { evidence_hash })
        }
        IbcEvent::ConnectionOpenInit(ConnectionOpenInit {
            connection_id,
            client_id,
            counterparty_client_id,
        }) => IbcEvent::ConnectionOpenInit(ConnectionOpenInit {
            connection_id,
            client_id,
            counterparty_client_id: counterparty_client_id.parse().unwrap(),
        }),
        IbcEvent::ConnectionOpenTry(ConnectionOpenTry {
            connection_id,
            client_id,
            counterparty_client_id,
            counterparty_connection_id,
        }) => IbcEvent::ConnectionOpenTry(ConnectionOpenTry {
            connection_id,
            client_id,
            counterparty_client_id: counterparty_client_id.parse().unwrap(),
            counterparty_connection_id,
        }),
        IbcEvent::ConnectionOpenAck(ConnectionOpenAck {
            connection_id,
            client_id,
            counterparty_client_id,
            counterparty_connection_id,
        }) => IbcEvent::ConnectionOpenAck(ConnectionOpenAck {
            connection_id,
            client_id,
            counterparty_client_id: counterparty_client_id.parse().unwrap(),
            counterparty_connection_id,
        }),
        IbcEvent::ConnectionOpenConfirm(ConnectionOpenConfirm {
            connection_id,
            client_id,
            counterparty_client_id,
            counterparty_connection_id,
        }) => IbcEvent::ConnectionOpenConfirm(ConnectionOpenConfirm {
            connection_id,
            client_id,
            counterparty_client_id: counterparty_client_id.parse().unwrap(),
            counterparty_connection_id,
        }),
        IbcEvent::ChannelOpenInit(ChannelOpenInit {
            port_id,
            channel_id,
            counterparty_port_id,
            connection_id,
            version,
        }) => IbcEvent::ChannelOpenInit(ChannelOpenInit {
            port_id,
            channel_id,
            counterparty_port_id,
            connection_id,
            version,
        }),
        IbcEvent::ChannelOpenTry(ChannelOpenTry {
            port_id,
            channel_id,
            counterparty_port_id,
            counterparty_channel_id,
            connection_id,
            version,
        }) => IbcEvent::ChannelOpenTry(ChannelOpenTry {
            port_id,
            channel_id,
            counterparty_port_id,
            counterparty_channel_id,
            connection_id,
            version,
        }),
        IbcEvent::ChannelOpenAck(ChannelOpenAck {
            port_id,
            channel_id,
            counterparty_port_id,
            counterparty_channel_id,
            connection_id,
        }) => IbcEvent::ChannelOpenAck(ChannelOpenAck {
            port_id,
            channel_id,
            counterparty_port_id,
            counterparty_channel_id,
            connection_id,
        }),
        IbcEvent::ChannelOpenConfirm(ChannelOpenConfirm {
            port_id,
            channel_id,
            counterparty_port_id,
            counterparty_channel_id,
            connection_id,
        }) => IbcEvent::ChannelOpenConfirm(ChannelOpenConfirm {
            port_id,
            channel_id,
            counterparty_port_id,
            counterparty_channel_id,
            connection_id,
        }),
        IbcEvent::WriteAcknowledgement(WriteAcknowledgement {
            packet_data_hex,
            packet_timeout_height,
            packet_timeout_timestamp,
            packet_sequence,
            packet_src_port,
            packet_src_channel,
            packet_dst_port,
            packet_dst_channel,
            packet_ack_hex,
            connection_id,
        }) => IbcEvent::WriteAcknowledgement(WriteAcknowledgement {
            packet_data_hex,
            packet_timeout_height,
            packet_timeout_timestamp,
            packet_sequence,
            packet_src_port,
            packet_src_channel,
            packet_dst_port,
            packet_dst_channel,
            packet_ack_hex,
            connection_id,
        }),
        IbcEvent::RecvPacket(RecvPacket {
            packet_data_hex,
            packet_timeout_height,
            packet_timeout_timestamp,
            packet_sequence,
            packet_src_port,
            packet_src_channel,
            packet_dst_port,
            packet_dst_channel,
            packet_channel_ordering,
            connection_id,
        }) => IbcEvent::RecvPacket(RecvPacket {
            packet_data_hex,
            packet_timeout_height,
            packet_timeout_timestamp,
            packet_sequence,
            packet_src_port,
            packet_src_channel,
            packet_dst_port,
            packet_dst_channel,
            packet_channel_ordering,
            connection_id,
        }),
        IbcEvent::SendPacket(SendPacket {
            packet_data_hex,
            packet_timeout_height,
            packet_timeout_timestamp,
            packet_sequence,
            packet_src_port,
            packet_src_channel,
            packet_dst_port,
            packet_dst_channel,
            packet_channel_ordering,
            connection_id,
        }) => IbcEvent::SendPacket(SendPacket {
            packet_data_hex,
            packet_timeout_height,
            packet_timeout_timestamp,
            packet_sequence,
            packet_src_port,
            packet_src_channel,
            packet_dst_port,
            packet_dst_channel,
            packet_channel_ordering,
            connection_id,
        }),
        IbcEvent::AcknowledgePacket(AcknowledgePacket {
            packet_timeout_height,
            packet_timeout_timestamp,
            packet_sequence,
            packet_src_port,
            packet_src_channel,
            packet_dst_port,
            packet_dst_channel,
            packet_channel_ordering,
            connection_id,
        }) => IbcEvent::AcknowledgePacket(AcknowledgePacket {
            packet_timeout_height,
            packet_timeout_timestamp,
            packet_sequence,
            packet_src_port,
            packet_src_channel,
            packet_dst_port,
            packet_dst_channel,
            packet_channel_ordering,
            connection_id,
        }),
        IbcEvent::TimeoutPacket(TimeoutPacket {
            packet_timeout_height,
            packet_timeout_timestamp,
            packet_sequence,
            packet_src_port,
            packet_src_channel,
            packet_dst_port,
            packet_dst_channel,
            packet_channel_ordering,
            connection_id,
        }) => IbcEvent::TimeoutPacket(TimeoutPacket {
            packet_timeout_height,
            packet_timeout_timestamp,
            packet_sequence,
            packet_src_port,
            packet_src_channel,
            packet_dst_port,
            packet_dst_channel,
            packet_channel_ordering,
            connection_id,
        }),
    }
}

pub enum ClientType {
    Wasm(WasmClientType),
    Tendermint,
}

#[cfg(test)]
mod tests {
    use std::marker::PhantomData;

    use block_message::BlockMessageTypes;
    use chain_utils::{
        cosmos::Cosmos, ethereum::Ethereum, scroll::Scroll, union::Union, wasm::Wasm,
    };
    use hex_literal::hex;
    use queue_msg::{
        aggregate, defer_relative, effect, event, fetch, repeat, seq, QueueMsg, QueueMsgTypes,
    };
    use relay_message::{
        aggregate::AggregateCreateClient,
        chain_impls::{
            cosmos_sdk::{
                fetch::{AbciQueryType, FetchAbciQuery},
                wasm::WasmConfig,
            },
            ethereum::EthereumConfig,
        },
        effect::{MsgChannelOpenInitData, MsgConnectionOpenInitData},
        event::IbcEvent,
        fetch::{FetchSelfClientState, FetchSelfConsensusState},
        RelayMessageTypes,
    };
    use unionlabs::{
        ethereum::config::Minimal,
        events::ConnectionOpenTry,
        hash::{H160, H256},
        ibc::core::{
            channel::{
                self, channel::Channel, msg_channel_open_init::MsgChannelOpenInit, order::Order,
            },
            commitment::merkle_prefix::MerklePrefix,
            connection::{self, msg_connection_open_init::MsgConnectionOpenInit, version::Version},
        },
        ics24,
        uint::U256,
        QueryHeight, DELAY_PERIOD,
    };

    use crate::{FromQueueMsg, VoyagerMessageTypes};

    macro_rules! parse {
        ($expr:expr) => {
            $expr.parse().unwrap()
        };
    }

    #[test]
    fn msg_serde() {
        let union_chain_id: String = parse!("union-devnet-1");
        let eth_chain_id: U256 = parse!("32382");
        let simd_chain_id: String = parse!("simd-devnet-1");
        let scroll_chain_id: U256 = parse!("534351");
        let stargaze_chain_id: String = parse!("stargaze-devnet-1");
        let osmosis_chain_id: String = parse!("osmosis-devnet-1");

        println!("---------------------------------------");
        println!("Union - Eth (Sending to Union) Connection Open: ");
        println!("---------------------------------------");
        print_json::<RelayMessageTypes>(effect(relay_message::id::<
            Wasm<Union>,
            Ethereum<Minimal>,
            _,
        >(
            union_chain_id.clone(),
            MsgConnectionOpenInitData(MsgConnectionOpenInit {
                client_id: parse!("08-wasm-0"),
                counterparty: connection::counterparty::Counterparty {
                    client_id: parse!("cometbls-0"),
                    connection_id: parse!(""),
                    prefix: MerklePrefix {
                        key_prefix: b"ibc".to_vec(),
                    },
                },
                version: Version {
                    identifier: "1".into(),
                    features: [Order::Ordered, Order::Unordered].into_iter().collect(),
                },
                delay_period: DELAY_PERIOD,
            }),
        )));

        println!("---------------------------------------");
        println!("Fetch Client State: ");
        println!("---------------------------------------");
        print_json::<RelayMessageTypes>(fetch(relay_message::id::<
            Wasm<Union>,
            Ethereum<Minimal>,
            _,
        >(
            union_chain_id.clone(),
            relay_message::fetch::Fetch::specific(FetchAbciQuery {
                path: ics24::Path::ClientState(ics24::ClientStatePath {
                    client_id: parse!("client-id"),
                }),
                height: parse!("123-456"),
                ty: AbciQueryType::State,
            }),
        )));

        println!("---------------------------------------");
        println!("Eth - Union (Sending to Union) Channel Open: ");
        println!("---------------------------------------");
        print_json::<RelayMessageTypes>(effect(relay_message::id::<
            Wasm<Union>,
            Ethereum<Minimal>,
            _,
        >(
            union_chain_id.clone(),
            MsgChannelOpenInitData {
                msg: MsgChannelOpenInit {
                    port_id: parse!("WASM_PORT_ID"),
                    channel: Channel {
                        state: channel::state::State::Init,
                        ordering: channel::order::Order::Unordered,
                        counterparty: channel::counterparty::Counterparty {
                            port_id: parse!("ucs01-relay"),
                            channel_id: parse!(""),
                        },
                        connection_hops: vec![parse!("connection-8")],
                        version: "ucs01-0".to_string(),
                    },
                },
                __marker: PhantomData,
            },
        )));

        println!("---------------------------------------");
        println!("Eth - Union (Starting on Union) Channel Open: ");
        println!("---------------------------------------");
        print_json::<RelayMessageTypes>(effect(relay_message::id::<
            Ethereum<Minimal>,
            Wasm<Union>,
            _,
        >(
            eth_chain_id,
            MsgChannelOpenInitData {
                msg: MsgChannelOpenInit {
                    port_id: parse!("ucs01-relay"),
                    channel: Channel {
                        state: channel::state::State::Init,
                        ordering: channel::order::Order::Ordered,
                        counterparty: channel::counterparty::Counterparty {
                            port_id: parse!("ucs01-relay"),
                            channel_id: parse!(""),
                        },
                        connection_hops: vec![parse!("connection-8")],
                        version: "ucs001-pingpong".to_string(),
                    },
                },
                __marker: PhantomData,
            },
        )));

        println!("---------------------------------------");
        println!("Eth - Union (Sending to Eth) Connection Open: ");
        println!("---------------------------------------");
        print_json::<RelayMessageTypes>(effect(relay_message::id::<
            Ethereum<Minimal>,
            Wasm<Union>,
            _,
        >(
            eth_chain_id,
            MsgConnectionOpenInitData(MsgConnectionOpenInit {
                client_id: parse!("cometbls-0"),
                counterparty: connection::counterparty::Counterparty {
                    client_id: parse!("08-wasm-0"),
                    connection_id: parse!(""),
                    prefix: MerklePrefix {
                        key_prefix: b"ibc".to_vec(),
                    },
                },
                version: Version {
                    identifier: "1".into(),
                    features: [Order::Ordered, Order::Unordered].into_iter().collect(),
                },
                delay_period: DELAY_PERIOD,
            }),
        )));

        println!("---------------------------------------");
        println!("Eth - Union (Sending to Eth) Connection Try: ");
        println!("---------------------------------------");
        print_json::<RelayMessageTypes>(event(relay_message::id::<
            Ethereum<Minimal>,
            Wasm<Union>,
            _,
        >(
            eth_chain_id,
            IbcEvent {
                tx_hash: H256([0; 32]),
                height: parse!("0-2941"),
                event: unionlabs::events::IbcEvent::ConnectionOpenTry(ConnectionOpenTry {
                    connection_id: parse!("connection-0"),
                    client_id: parse!("cometbls-0"),
                    counterparty_client_id: parse!("08-wasm-1"),
                    counterparty_connection_id: parse!("connection-14"),
                }),
            },
        )));

        println!("---------------------------------------");
        println!("Eth - Union (Sending to Eth) Update Client: ");
        println!("---------------------------------------");
        print_json::<RelayMessageTypes>(repeat(
            None,
            seq([
                event(relay_message::id::<Ethereum<Minimal>, Wasm<Union>, _>(
                    eth_chain_id,
                    relay_message::event::Command::UpdateClient {
                        client_id: parse!("cometbls-0"),
                        __marker: PhantomData,
                    },
                )),
                defer_relative(10),
            ]),
        ));

        println!("---------------------------------------");
        println!("Eth - Union (Sending to Union) Update Client: ");
        println!("---------------------------------------");
        print_json::<RelayMessageTypes>(repeat(
            None,
            seq([
                event(relay_message::id::<Wasm<Union>, Ethereum<Minimal>, _>(
                    union_chain_id.clone(),
                    relay_message::event::Command::UpdateClient {
                        client_id: parse!("08-wasm-0"),
                        __marker: PhantomData,
                    },
                )),
                defer_relative(10),
            ]),
        ));

        println!("---------------------------------------");
        println!("Cosmos - Union (Sending to Cosmos) Update Client: ");
        println!("---------------------------------------");
        print_json::<RelayMessageTypes>(repeat(
            None,
            seq([
                event(relay_message::id::<Wasm<Cosmos>, Wasm<Union>, _>(
                    simd_chain_id.clone(),
                    relay_message::event::Command::UpdateClient {
                        client_id: parse!("08-wasm-0"),
                        __marker: PhantomData,
                    },
                )),
                defer_relative(10),
            ]),
        ));

        println!("---------------------------------------");
        println!("Cosmos - Union (Sending to Union) Update Client: ");
        println!("---------------------------------------");
        print_json::<RelayMessageTypes>(repeat(
            None,
            seq([
                event(relay_message::id::<Wasm<Union>, Wasm<Cosmos>, _>(
                    union_chain_id.clone(),
                    relay_message::event::Command::UpdateClient {
                        client_id: parse!("07-tendermint-0"),
                        __marker: PhantomData,
                    },
                )),
                defer_relative(10),
            ]),
        ));

        println!("---------------------------------------");
        println!("Scroll - Union (Sending to Union) Create Scroll lightclient on Union: ");
        println!("---------------------------------------");
        print_json::<RelayMessageTypes>(aggregate(
            [
                fetch(relay_message::id::<Scroll, Wasm<Union>, _>(
                    scroll_chain_id,
                    FetchSelfClientState {
                        at: QueryHeight::Latest,
                        __marker: PhantomData,
                    },
                )),
                fetch(relay_message::id::<Scroll, Wasm<Union>, _>(
                    scroll_chain_id,
                    FetchSelfConsensusState {
                        at: QueryHeight::Latest,
                        __marker: PhantomData,
                    },
                )),
            ],
            [],
            relay_message::id::<Wasm<Union>, Scroll, _>(
                union_chain_id.clone(),
                AggregateCreateClient {
                    config: WasmConfig {
                        checksum: H256(hex!(
                            "c4c38c95b12a03dabe366dab1a19671193b5f8de7abf53eb3ecabbb946a4ac88"
                        )),
                    },
                    __marker: PhantomData,
                },
            ),
        ));

        println!("---------------------------------------");
        println!("Scroll - single update client");
        println!("---------------------------------------");
        print_json::<RelayMessageTypes>(event(relay_message::id::<Scroll, Wasm<Union>, _>(
            scroll_chain_id,
            relay_message::event::Command::UpdateClient {
                client_id: parse!("cometbls-0"),
                __marker: PhantomData,
            },
        )));

        println!("---------------------------------------");
        println!("Union - Eth Create Both Clients: ");
        println!("---------------------------------------");
        print_json::<RelayMessageTypes>(seq([
            aggregate(
                [
                    fetch(relay_message::id::<Wasm<Union>, Ethereum<Minimal>, _>(
                        union_chain_id.clone(),
                        FetchSelfClientState {
                            at: QueryHeight::Latest,
                            __marker: PhantomData,
                        },
                    )),
                    fetch(relay_message::id::<Wasm<Union>, Ethereum<Minimal>, _>(
                        union_chain_id.clone(),
                        FetchSelfConsensusState {
                            at: QueryHeight::Latest,
                            __marker: PhantomData,
                        },
                    )),
                ],
                [],
                relay_message::id::<Ethereum<Minimal>, Wasm<Union>, _>(
                    eth_chain_id,
                    AggregateCreateClient {
                        config: EthereumConfig {
                            client_type: "cometbls".to_string(),
                            client_address: H160(hex!("83428c7db9815f482a39a1715684dcf755021997")),
                        },
                        __marker: PhantomData,
                    },
                ),
            ),
            aggregate(
                [
                    fetch(relay_message::id::<Ethereum<Minimal>, Wasm<Union>, _>(
                        eth_chain_id,
                        FetchSelfClientState {
                            at: QueryHeight::Latest,
                            __marker: PhantomData,
                        },
                    )),
                    fetch(relay_message::id::<Ethereum<Minimal>, Wasm<Union>, _>(
                        eth_chain_id,
                        FetchSelfConsensusState {
                            at: QueryHeight::Latest,
                            __marker: PhantomData,
                        },
                    )),
                ],
                [],
                relay_message::id::<Wasm<Union>, Ethereum<Minimal>, _>(
                    union_chain_id.clone(),
                    AggregateCreateClient {
                        config: WasmConfig {
                            checksum: H256(hex!(
                                "78266014ea77f3b785e45a33d1f8d3709444a076b3b38b2aeef265b39ad1e494"
                            )),
                        },
                        __marker: PhantomData,
                    },
                ),
            ),
        ]));

        println!("---------------------------------------");
        println!("Union - Cosmos Create Both Client: ");
        println!("---------------------------------------");
        print_json::<RelayMessageTypes>(seq([
            aggregate(
                [
                    fetch(relay_message::id::<Wasm<Cosmos>, Wasm<Union>, _>(
                        simd_chain_id.clone(),
                        FetchSelfClientState {
                            at: QueryHeight::Latest,
                            __marker: PhantomData,
                        },
                    )),
                    fetch(relay_message::id::<Wasm<Cosmos>, Wasm<Union>, _>(
                        simd_chain_id.clone(),
                        FetchSelfConsensusState {
                            at: QueryHeight::Latest,
                            __marker: PhantomData,
                        },
                    )),
                ],
                [],
                relay_message::id::<Wasm<Union>, Wasm<Cosmos>, _>(
                    union_chain_id.clone(),
                    AggregateCreateClient {
                        config: WasmConfig {
                            checksum: H256(hex!(
                                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                            )),
                        },
                        __marker: PhantomData,
                    },
                ),
            ),
            aggregate(
                [
                    fetch(relay_message::id::<Wasm<Union>, Wasm<Cosmos>, _>(
                        union_chain_id.clone(),
                        FetchSelfClientState {
                            at: QueryHeight::Latest,
                            __marker: PhantomData,
                        },
                    )),
                    fetch(relay_message::id::<Wasm<Union>, Wasm<Cosmos>, _>(
                        union_chain_id.clone(),
                        FetchSelfConsensusState {
                            at: QueryHeight::Latest,
                            __marker: PhantomData,
                        },
                    )),
                ],
                [],
                relay_message::id::<Wasm<Cosmos>, Wasm<Union>, _>(
                    simd_chain_id,
                    AggregateCreateClient {
                        config: WasmConfig {
                            checksum: H256(hex!(
                                "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                            )),
                        },
                        __marker: PhantomData,
                    },
                ),
            ),
        ]));

        println!("---------------------------------------");
        println!("Cosmos - Cosmos Create Both Client: ");
        println!("---------------------------------------");
        print_json::<RelayMessageTypes>(seq([
            aggregate(
                [
                    fetch(relay_message::id::<Cosmos, Cosmos, _>(
                        stargaze_chain_id.clone(),
                        FetchSelfClientState {
                            at: QueryHeight::Latest,
                            __marker: PhantomData,
                        },
                    )),
                    fetch(relay_message::id::<Cosmos, Cosmos, _>(
                        stargaze_chain_id.clone(),
                        FetchSelfConsensusState {
                            at: QueryHeight::Latest,
                            __marker: PhantomData,
                        },
                    )),
                ],
                [],
                relay_message::id::<Cosmos, Cosmos, _>(
                    osmosis_chain_id.clone(),
                    AggregateCreateClient {
                        config: (),
                        __marker: PhantomData,
                    },
                ),
            ),
            aggregate(
                [
                    fetch(relay_message::id::<Cosmos, Cosmos, _>(
                        osmosis_chain_id.clone(),
                        FetchSelfClientState {
                            at: QueryHeight::Latest,
                            __marker: PhantomData,
                        },
                    )),
                    fetch(relay_message::id::<Cosmos, Cosmos, _>(
                        osmosis_chain_id.clone(),
                        FetchSelfConsensusState {
                            at: QueryHeight::Latest,
                            __marker: PhantomData,
                        },
                    )),
                ],
                [],
                relay_message::id::<Cosmos, Cosmos, _>(
                    stargaze_chain_id.clone(),
                    AggregateCreateClient {
                        config: (),
                        __marker: PhantomData,
                    },
                ),
            ),
        ]));

        println!("---------------------------------------");
        println!("Scroll - single update client");
        println!("---------------------------------------");
        print_json::<RelayMessageTypes>(event(relay_message::id::<Scroll, Wasm<Union>, _>(
            scroll_chain_id,
            relay_message::event::Command::UpdateClient {
                client_id: parse!("cometbls-0"),
                __marker: PhantomData,
            },
        )));

        print_json::<BlockMessageTypes>(fetch(block_message::id::<Cosmos, _>(
            "simd-devnet-1".parse().unwrap(),
            block_message::fetch::FetchBlock {
                height: unionlabs::ibc::core::client::height::Height {
                    revision_number: 1,
                    revision_height: 1,
                },
            },
        )));

        print_json::<BlockMessageTypes>(fetch(block_message::id::<Union, _>(
            "union-devnet-1".parse().unwrap(),
            block_message::fetch::FetchBlock {
                height: unionlabs::ibc::core::client::height::Height {
                    revision_number: 1,
                    revision_height: 1,
                },
            },
        )));
    }

    fn print_json<T: QueueMsgTypes>(msg: QueueMsg<T>)
    where
        VoyagerMessageTypes: FromQueueMsg<T>,
    {
        let msg = VoyagerMessageTypes::from_queue_msg(msg);

        let json = serde_json::to_string(&msg).unwrap();

        println!("{json}\n");

        let from_json = serde_json::from_str(&json).unwrap();

        assert_eq!(&msg, &from_json, "json roundtrip failed");
    }
}
