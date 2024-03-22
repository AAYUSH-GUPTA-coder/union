use std::marker::PhantomData;

use chain_utils::cosmos_sdk::{BroadcastTxCommitError, CosmosSdkChain, CosmosSdkChainExt};
use prost::Message;
use queue_msg::{data, fetch, seq, wait, QueueMsg};
use unionlabs::{
    encoding::{Decode, Encode, Proto},
    google::protobuf::any::{mk_any, IntoAny},
    ibc::core::{client::height::IsHeight, commitment::merkle_proof::MerkleProof},
    proof::{ClientStatePath, Path},
    traits::{ClientStateOf, ConsensusStateOf, HeaderOf, HeightOf},
    TypeUrl,
};

use crate::{
    chain_impls::cosmos_sdk::fetch::{AbciQueryType, FetchAbciQuery},
    data::{AnyData, Data, IbcProof, IbcState},
    fetch::{AnyFetch, Fetch},
    id, identified,
    msg::{
        Msg, MsgAckPacketData, MsgChannelOpenAckData, MsgChannelOpenConfirmData,
        MsgChannelOpenInitData, MsgChannelOpenTryData, MsgConnectionOpenAckData,
        MsgConnectionOpenConfirmData, MsgConnectionOpenInitData, MsgConnectionOpenTryData,
        MsgCreateClientData, MsgRecvPacketData, MsgUpdateClientData,
    },
    use_aggregate::IsAggregateData,
    wait::{AnyWait, Wait, WaitForBlock},
    AnyLightClientIdentified, ChainExt, DoFetchProof, DoFetchState, Identified, PathOf,
    RelayerMsgTypes,
};

pub trait CosmosSdkChainSealed: CosmosSdkChain + ChainExt {}

pub async fn do_msg<Hc, Tr>(
    hc: &Hc,
    msg: Msg<Hc, Tr>,
    // We need to be able to customize the encoding of the client/consensus states and client messages (header) since Wasm<_> needs to wrap them in wasm.v1.*; but since the rest of the logic is exactly the same, the following two functions are used as hooks to allow for the behaviour to be otherwise reused.
    mk_create_client_states: fn(
        Hc::Config,
        ClientStateOf<Tr>,
        ConsensusStateOf<Tr>,
    )
        -> (protos::google::protobuf::Any, protos::google::protobuf::Any),
    mk_client_message: fn(Tr::Header) -> protos::google::protobuf::Any,
) -> Result<(), BroadcastTxCommitError>
where
    Hc: CosmosSdkChainSealed<MsgError = BroadcastTxCommitError>,
    Tr: ChainExt,

    ConsensusStateOf<Tr>: Encode<Proto> + TypeUrl,
    ClientStateOf<Tr>: Encode<Proto> + TypeUrl,
    HeaderOf<Tr>: Encode<Proto> + TypeUrl,

    ConsensusStateOf<Hc>: Encode<Proto> + TypeUrl,
    ClientStateOf<Hc>: Encode<Proto> + TypeUrl,

    Tr::StoredClientState<Hc>: IntoAny,
    Tr::StateProof: Encode<Proto>,
{
    hc.signers()
        .with(|signer| async {
            let msg_any = match msg.clone() {
                Msg::ConnectionOpenInit(MsgConnectionOpenInitData(data)) => {
                    mk_any(&protos::ibc::core::connection::v1::MsgConnectionOpenInit {
                        client_id: data.client_id.to_string(),
                        counterparty: Some(data.counterparty.into()),
                        version: Some(data.version.into()),
                        signer: signer.to_string(),
                        delay_period: data.delay_period,
                    })
                }
                Msg::ConnectionOpenTry(MsgConnectionOpenTryData(data)) => {
                    mk_any(&protos::ibc::core::connection::v1::MsgConnectionOpenTry {
                        client_id: data.client_id.to_string(),
                        client_state: Some(data.client_state.into_any().into()),
                        counterparty: Some(data.counterparty.into()),
                        delay_period: data.delay_period,
                        counterparty_versions: data
                            .counterparty_versions
                            .into_iter()
                            .map(Into::into)
                            .collect(),
                        proof_height: Some(data.proof_height.into_height().into()),
                        proof_init: data.proof_init.encode(),
                        proof_client: data.proof_client.encode(),
                        proof_consensus: data.proof_consensus.encode(),
                        consensus_height: Some(data.consensus_height.into_height().into()),
                        signer: signer.to_string(),
                        host_consensus_state_proof: vec![],
                        ..Default::default()
                    })
                }
                Msg::ConnectionOpenAck(MsgConnectionOpenAckData(data)) => {
                    mk_any(&protos::ibc::core::connection::v1::MsgConnectionOpenAck {
                        client_state: Some(data.client_state.into_any().into()),
                        proof_height: Some(data.proof_height.into_height().into()),
                        proof_client: data.proof_client.encode(),
                        proof_consensus: data.proof_consensus.encode(),
                        consensus_height: Some(data.consensus_height.into_height().into()),
                        signer: signer.to_string(),
                        host_consensus_state_proof: vec![],
                        connection_id: data.connection_id.to_string(),
                        counterparty_connection_id: data.counterparty_connection_id.to_string(),
                        version: Some(data.version.into()),
                        proof_try: data.proof_try.encode(),
                    })
                }
                Msg::ConnectionOpenConfirm(MsgConnectionOpenConfirmData { msg, __marker }) => {
                    mk_any(
                        &protos::ibc::core::connection::v1::MsgConnectionOpenConfirm {
                            connection_id: msg.connection_id.to_string(),
                            proof_ack: msg.proof_ack.encode(),
                            proof_height: Some(msg.proof_height.into_height().into()),
                            signer: signer.to_string(),
                        },
                    )
                }
                Msg::ChannelOpenInit(MsgChannelOpenInitData { msg, __marker }) => {
                    mk_any(&protos::ibc::core::channel::v1::MsgChannelOpenInit {
                        port_id: msg.port_id.to_string(),
                        channel: Some(msg.channel.into()),
                        signer: signer.to_string(),
                    })
                }
                Msg::ChannelOpenTry(MsgChannelOpenTryData { msg, __marker }) => {
                    mk_any(&protos::ibc::core::channel::v1::MsgChannelOpenTry {
                        port_id: msg.port_id.to_string(),
                        channel: Some(msg.channel.into()),
                        counterparty_version: msg.counterparty_version,
                        proof_init: msg.proof_init.encode(),
                        proof_height: Some(msg.proof_height.into()),
                        signer: signer.to_string(),
                        ..Default::default()
                    })
                }
                Msg::ChannelOpenAck(MsgChannelOpenAckData { msg, __marker }) => {
                    mk_any(&protos::ibc::core::channel::v1::MsgChannelOpenAck {
                        port_id: msg.port_id.to_string(),
                        channel_id: msg.channel_id.to_string(),
                        counterparty_version: msg.counterparty_version,
                        counterparty_channel_id: msg.counterparty_channel_id.to_string(),
                        proof_try: msg.proof_try.encode(),
                        proof_height: Some(msg.proof_height.into_height().into()),
                        signer: signer.to_string(),
                    })
                }
                Msg::ChannelOpenConfirm(MsgChannelOpenConfirmData { msg, __marker }) => {
                    mk_any(&protos::ibc::core::channel::v1::MsgChannelOpenConfirm {
                        port_id: msg.port_id.to_string(),
                        channel_id: msg.channel_id.to_string(),
                        proof_height: Some(msg.proof_height.into_height().into()),
                        signer: signer.to_string(),
                        proof_ack: msg.proof_ack.encode(),
                    })
                }
                Msg::RecvPacket(MsgRecvPacketData { msg, __marker }) => {
                    mk_any(&protos::ibc::core::channel::v1::MsgRecvPacket {
                        packet: Some(msg.packet.into()),
                        proof_height: Some(msg.proof_height.into_height().into()),
                        signer: signer.to_string(),
                        proof_commitment: msg.proof_commitment.encode(),
                    })
                }
                Msg::AckPacket(MsgAckPacketData { msg, __marker }) => {
                    mk_any(&protos::ibc::core::channel::v1::MsgAcknowledgement {
                        packet: Some(msg.packet.into()),
                        acknowledgement: msg.acknowledgement,
                        proof_acked: msg.proof_acked.encode(),
                        proof_height: Some(msg.proof_height.into_height().into()),
                        signer: signer.to_string(),
                    })
                }
                Msg::CreateClient(MsgCreateClientData { msg, config }) => {
                    let (client_state, consensus_state) =
                        mk_create_client_states(config, msg.client_state, msg.consensus_state);

                    mk_any(&protos::ibc::core::client::v1::MsgCreateClient {
                        client_state: Some(client_state),
                        consensus_state: Some(consensus_state),
                        signer: signer.to_string(),
                    })
                }
                Msg::UpdateClient(MsgUpdateClientData(msg)) => {
                    mk_any(&protos::ibc::core::client::v1::MsgUpdateClient {
                        signer: signer.to_string(),
                        client_id: msg.client_id.to_string(),
                        client_message: Some(mk_client_message(msg.client_message)),
                    })
                }
            };

            let tx_hash = hc.broadcast_tx_commit(signer, [msg_any]).await?;

            tracing::info!("cosmos tx {:?} => {:?}", tx_hash, msg);

            Ok(())
        })
        .await
}

impl<Hc, Tr> DoFetchState<Hc, Tr> for Hc
where
    Hc: CosmosSdkChainSealed + ChainExt<StateProof = MerkleProof>,
    Tr: ChainExt,

    Hc::Fetch<Tr>: From<FetchAbciQuery<Hc, Tr>>,

    AnyLightClientIdentified<AnyFetch>: From<identified!(Fetch<Hc, Tr>)>,
    AnyLightClientIdentified<AnyWait>: From<identified!(Wait<Hc, Tr>)>,
    // required by fetch_abci_query, can be removed once that's been been removed
    AnyLightClientIdentified<AnyData>: From<identified!(Data<Hc, Tr>)>,
    Tr::SelfClientState: Decode<Proto>,
    Tr::SelfConsensusState: Decode<Proto>,

    Hc::StoredClientState<Tr>: Decode<Proto>,
    Hc::StoredConsensusState<Tr>: Decode<Proto>,

    Identified<Hc, Tr, IbcState<ClientStatePath<Hc::ClientId>, Hc, Tr>>: IsAggregateData,
{
    fn state(hc: &Hc, at: HeightOf<Hc>, path: PathOf<Hc, Tr>) -> QueueMsg<RelayerMsgTypes> {
        seq([
            wait(id(
                hc.chain_id(),
                WaitForBlock {
                    // height: at.increment(),
                    height: at,
                    __marker: PhantomData,
                },
            )),
            fetch(id::<Hc, Tr, _>(
                hc.chain_id(),
                Fetch::specific(FetchAbciQuery {
                    path,
                    height: at,
                    ty: AbciQueryType::State,
                }),
            )),
        ])
    }

    async fn query_client_state(
        hc: &Hc,
        client_id: Hc::ClientId,
        height: Hc::Height,
    ) -> Hc::StoredClientState<Tr> {
        let QueueMsg::Data(relayer_msg) = fetch_abci_query::<Hc, Tr>(
            hc,
            ClientStatePath { client_id }.into(),
            height,
            AbciQueryType::State,
        )
        .await
        else {
            panic!()
        };

        Identified::<Hc, Tr, IbcState<ClientStatePath<Hc::ClientId>, Hc, Tr>>::try_from(relayer_msg)
            .unwrap()
            .t
            .state
    }
}

impl<Hc, Tr> DoFetchProof<Hc, Tr> for Hc
where
    Hc: ChainExt + CosmosSdkChainSealed,
    Tr: ChainExt,
    Hc::Fetch<Tr>: From<FetchAbciQuery<Hc, Tr>>,
    AnyLightClientIdentified<AnyFetch>: From<identified!(Fetch<Hc, Tr>)>,
    AnyLightClientIdentified<AnyWait>: From<identified!(Wait<Hc, Tr>)>,
{
    fn proof(hc: &Hc, at: HeightOf<Hc>, path: PathOf<Hc, Tr>) -> QueueMsg<RelayerMsgTypes> {
        seq([
            wait(id(
                hc.chain_id(),
                WaitForBlock {
                    height: at,
                    __marker: PhantomData,
                },
            )),
            fetch(id::<Hc, Tr, _>(
                hc.chain_id(),
                Fetch::specific(FetchAbciQuery::<Hc, Tr> {
                    path,
                    height: at,
                    ty: AbciQueryType::Proof,
                }),
            )),
        ])
    }
}

pub async fn fetch_abci_query<Hc, Tr>(
    c: &Hc,
    path: Path<Hc::ClientId, Tr::Height>,
    height: HeightOf<Hc>,
    ty: AbciQueryType,
) -> QueueMsg<RelayerMsgTypes>
where
    Hc: CosmosSdkChain + ChainExt<StateProof = MerkleProof>,
    Tr: ChainExt,
    AnyLightClientIdentified<AnyData>: From<identified!(Data<Hc, Tr>)>,
    Identified<Hc, Tr, IbcState<ClientStatePath<Hc::ClientId>, Hc, Tr>>: IsAggregateData,

    Hc::StoredClientState<Tr>: Decode<Proto>,
    Hc::StoredConsensusState<Tr>: Decode<Proto>,
{
    let mut client =
        protos::cosmos::base::tendermint::v1beta1::service_client::ServiceClient::connect(
            c.grpc_url().clone(),
        )
        .await
        .unwrap();

    let query_result = client
        .abci_query(
            protos::cosmos::base::tendermint::v1beta1::AbciQueryRequest {
                data: path.to_string().into_bytes(),
                path: "store/ibc/key".to_string(),
                height: i64::try_from(height.revision_height()).unwrap() - 1_i64,
                prove: matches!(ty, AbciQueryType::Proof),
            },
        )
        .await
        .unwrap()
        .into_inner();

    match ty {
        AbciQueryType::State => match path {
            Path::ClientStatePath(path) => data(id::<Hc, Tr, _>(
                c.chain_id(),
                IbcState::<ClientStatePath<Hc::ClientId>, Hc, Tr> {
                    height,
                    state: Hc::StoredClientState::<Tr>::decode(&query_result.value).unwrap(),
                    path,
                },
            )),
            Path::ClientConsensusStatePath(path) => data(id::<Hc, Tr, _>(
                c.chain_id(),
                IbcState {
                    height,
                    state: Hc::StoredConsensusState::<Tr>::decode(&query_result.value).unwrap(),
                    path,
                },
            )),
            Path::ConnectionPath(path) => data(id::<Hc, Tr, _>(
                c.chain_id(),
                IbcState {
                    height,
                    state: Decode::<unionlabs::encoding::Proto>::decode(&query_result.value)
                        .unwrap(),
                    path,
                },
            )),
            Path::ChannelEndPath(path) => data(id::<Hc, Tr, _>(
                c.chain_id(),
                IbcState {
                    height,
                    state: Decode::<unionlabs::encoding::Proto>::decode(&query_result.value)
                        .unwrap(),
                    path,
                },
            )),
            Path::CommitmentPath(path) => data(id::<Hc, Tr, _>(
                c.chain_id(),
                IbcState {
                    height,
                    state: query_result.value.try_into().unwrap(),
                    path,
                },
            )),
            Path::AcknowledgementPath(path) => data(id::<Hc, Tr, _>(
                c.chain_id(),
                IbcState {
                    height,
                    state: query_result.value.try_into().unwrap(),
                    path,
                },
            )),
        },
        AbciQueryType::Proof => {
            let proof = MerkleProof::try_from(protos::ibc::core::commitment::v1::MerkleProof {
                proofs: query_result
                    .proof_ops
                    .unwrap()
                    .ops
                    .into_iter()
                    .map(|op| {
                        protos::cosmos::ics23::v1::CommitmentProof::decode(op.data.as_slice())
                            .unwrap()
                    })
                    .collect::<Vec<_>>(),
            })
            .unwrap();

            match path {
                Path::ClientStatePath(path) => data(id::<Hc, Tr, _>(
                    c.chain_id(),
                    IbcProof::<_, Hc, Tr> {
                        proof,
                        height,
                        path,
                        __marker: PhantomData,
                    },
                )),
                Path::ClientConsensusStatePath(path) => data(id::<Hc, Tr, _>(
                    c.chain_id(),
                    IbcProof::<_, Hc, Tr> {
                        proof,
                        height,
                        path,
                        __marker: PhantomData,
                    },
                )),
                Path::ConnectionPath(path) => data(id::<Hc, Tr, _>(
                    c.chain_id(),
                    IbcProof::<_, Hc, Tr> {
                        proof,
                        height,
                        path,
                        __marker: PhantomData,
                    },
                )),
                Path::ChannelEndPath(path) => data(id::<Hc, Tr, _>(
                    c.chain_id(),
                    IbcProof::<_, Hc, Tr> {
                        proof,
                        height,
                        path,
                        __marker: PhantomData,
                    },
                )),
                Path::CommitmentPath(path) => data(id::<Hc, Tr, _>(
                    c.chain_id(),
                    IbcProof::<_, Hc, Tr> {
                        proof,
                        height,
                        path,
                        __marker: PhantomData,
                    },
                )),
                Path::AcknowledgementPath(path) => data(id::<Hc, Tr, _>(
                    c.chain_id(),
                    IbcProof::<_, Hc, Tr> {
                        proof,
                        height,
                        path,
                        __marker: PhantomData,
                    },
                )),
            }
        }
    }
}

pub mod fetch {
    use std::marker::PhantomData;

    use chain_utils::cosmos_sdk::CosmosSdkChain;
    use frame_support_procedural::{CloneNoBound, DebugNoBound, PartialEqNoBound};
    use macros::apply;
    use queue_msg::{data, msg_struct, QueueMsg};
    use serde::{Deserialize, Serialize};
    use tendermint_rpc::Client;
    use unionlabs::{ibc::core::client::height::IsHeight, traits::HeightOf};

    use crate::{
        chain_impls::cosmos_sdk::{
            data::{TrustedCommit, TrustedValidators, UntrustedCommit, UntrustedValidators},
            tendermint_helpers::{
                tendermint_commit_to_signed_header, tendermint_validator_info_to_validator,
            },
        },
        data::{AnyData, Data},
        id, identified, AnyLightClientIdentified, ChainExt, PathOf, RelayerMsgTypes,
    };

    #[apply(msg_struct)]
    pub struct FetchAbciQuery<Hc: ChainExt, Tr: ChainExt> {
        pub path: PathOf<Hc, Tr>,
        pub height: HeightOf<Hc>,
        pub ty: AbciQueryType,
    }

    #[derive(DebugNoBound, CloneNoBound, PartialEqNoBound, Serialize, Deserialize)]
    #[serde(
        bound(serialize = "", deserialize = ""),
        deny_unknown_fields,
        rename_all = "snake_case"
    )]
    #[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
    pub enum AbciQueryType {
        State,
        Proof,
    }

    #[apply(msg_struct)]
    #[cover(Tr)]
    pub struct FetchTrustedCommit<Hc: ChainExt, Tr: ChainExt> {
        pub height: Hc::Height,
    }

    #[apply(msg_struct)]
    #[cover(Tr)]
    pub struct FetchUntrustedCommit<Hc: ChainExt, Tr: ChainExt> {
        pub height: Hc::Height,
    }

    #[apply(msg_struct)]
    #[cover(Tr)]
    pub struct FetchTrustedValidators<Hc: ChainExt, Tr: ChainExt> {
        pub height: Hc::Height,
    }

    #[apply(msg_struct)]
    #[cover(Tr)]
    pub struct FetchUntrustedValidators<Hc: ChainExt, Tr: ChainExt> {
        pub height: Hc::Height,
    }

    pub async fn fetch_trusted_commit<Hc, Tr>(
        hc: &Hc,
        height: Hc::Height,
    ) -> QueueMsg<RelayerMsgTypes>
    where
        Hc: CosmosSdkChain + ChainExt,
        <Hc as ChainExt>::Data<Tr>: From<TrustedCommit<Hc, Tr>>,
        Tr: ChainExt,
        AnyLightClientIdentified<AnyData>: From<identified!(Data<Hc, Tr>)>,
    {
        let commit = hc
            .tm_client()
            .commit(
                TryInto::<::tendermint::block::Height>::try_into(height.revision_height()).unwrap(),
            )
            .await
            .unwrap();

        let signed_header = tendermint_commit_to_signed_header(commit);

        data(id::<Hc, Tr, _>(
            hc.chain_id(),
            Data::specific(TrustedCommit {
                height,
                // REVIEW: Ensure `commit.canonical`?
                signed_header,
                __marker: PhantomData,
            }),
        ))
    }

    pub async fn fetch_untrusted_commit<Hc, Tr>(
        hc: &Hc,
        height: Hc::Height,
    ) -> QueueMsg<RelayerMsgTypes>
    where
        Hc: CosmosSdkChain + ChainExt,
        <Hc as ChainExt>::Data<Tr>: From<UntrustedCommit<Hc, Tr>>,
        Tr: ChainExt,
        AnyLightClientIdentified<AnyData>: From<identified!(Data<Hc, Tr>)>,
    {
        let commit = hc
            .tm_client()
            .commit(
                TryInto::<::tendermint::block::Height>::try_into(height.revision_height()).unwrap(),
            )
            .await
            .unwrap();

        let signed_header = tendermint_commit_to_signed_header(commit);

        data(id::<Hc, Tr, _>(
            hc.chain_id(),
            Data::specific(UntrustedCommit {
                height,
                // REVIEW: Ensure `commit.canonical`?
                signed_header,
                __marker: PhantomData,
            }),
        ))
    }

    pub async fn fetch_trusted_validators<Hc, Tr>(
        hc: &Hc,
        height: Hc::Height,
    ) -> QueueMsg<RelayerMsgTypes>
    where
        Hc: CosmosSdkChain + ChainExt,
        <Hc as ChainExt>::Data<Tr>: From<TrustedValidators<Hc, Tr>>,
        Tr: ChainExt,
        AnyLightClientIdentified<AnyData>: From<identified!(Data<Hc, Tr>)>,
    {
        let validators = hc
            .tm_client()
            .validators(
                TryInto::<::tendermint::block::Height>::try_into(height.revision_height()).unwrap(),
                tendermint_rpc::Paging::All,
            )
            .await
            .unwrap()
            .validators
            .into_iter()
            .map(tendermint_validator_info_to_validator)
            .collect();

        data(id::<Hc, Tr, _>(
            hc.chain_id(),
            Data::specific(TrustedValidators {
                height,
                validators,
                __marker: PhantomData,
            }),
        ))
    }

    pub async fn fetch_untrusted_validators<Hc, Tr>(
        hc: &Hc,
        height: Hc::Height,
    ) -> QueueMsg<RelayerMsgTypes>
    where
        Hc: CosmosSdkChain + ChainExt,
        <Hc as ChainExt>::Data<Tr>: From<UntrustedValidators<Hc, Tr>>,
        Tr: ChainExt,
        AnyLightClientIdentified<AnyData>: From<identified!(Data<Hc, Tr>)>,
    {
        let validators = hc
            .tm_client()
            .validators(
                TryInto::<::tendermint::block::Height>::try_into(height.revision_height()).unwrap(),
                tendermint_rpc::Paging::All,
            )
            .await
            .unwrap()
            .validators
            .into_iter()
            .map(tendermint_validator_info_to_validator)
            .collect();

        data(id::<Hc, Tr, _>(
            hc.chain_id(),
            Data::specific(UntrustedValidators {
                height,
                validators,
                __marker: PhantomData,
            }),
        ))
    }
}

pub mod data {
    use macros::apply;
    use queue_msg::msg_struct;
    use unionlabs::tendermint::types::{signed_header::SignedHeader, validator::Validator};

    use crate::ChainExt;

    #[apply(msg_struct)]
    #[cover(Tr)]
    pub struct UntrustedCommit<Hc: ChainExt, Tr: ChainExt> {
        pub height: Hc::Height,
        pub signed_header: SignedHeader,
    }

    #[apply(msg_struct)]
    #[cover(Tr)]
    pub struct TrustedCommit<Hc: ChainExt, Tr: ChainExt> {
        pub height: Hc::Height,
        pub signed_header: SignedHeader,
    }

    #[apply(msg_struct)]
    #[cover(Hc, Tr)]
    pub struct TrustedValidators<Hc: ChainExt, Tr: ChainExt> {
        pub height: Hc::Height,
        pub validators: Vec<Validator>,
    }

    #[apply(msg_struct)]
    #[cover(Hc, Tr)]
    pub struct UntrustedValidators<Hc: ChainExt, Tr: ChainExt> {
        pub height: Hc::Height,
        pub validators: Vec<Validator>,
    }
}

pub mod tendermint_helpers {
    use unionlabs::{
        bounded::BoundedI64,
        google::protobuf::timestamp::Timestamp,
        hash::H256,
        tendermint::{
            crypto::public_key::PublicKey,
            types::{
                block_id::BlockId, commit::Commit, commit_sig::CommitSig,
                part_set_header::PartSetHeader, signed_header::SignedHeader, validator::Validator,
            },
        },
    };

    pub fn tendermint_commit_to_signed_header(
        commit: tendermint_rpc::endpoint::commit::Response,
    ) -> SignedHeader {
        let header_timestamp =
            tendermint_proto::google::protobuf::Timestamp::from(commit.signed_header.header.time);

        SignedHeader {
            header: unionlabs::tendermint::types::header::Header {
                version: unionlabs::tendermint::version::consensus::Consensus {
                    block: commit.signed_header.header.version.block,
                    app: commit.signed_header.header.version.app,
                },
                chain_id: commit.signed_header.header.chain_id.into(),
                height: tendermint_height_to_bounded_i64(commit.signed_header.header.height),
                time: Timestamp {
                    seconds: header_timestamp.seconds.try_into().unwrap(),
                    nanos: header_timestamp.nanos.try_into().unwrap(),
                },
                last_block_id: BlockId {
                    hash: tendermint_hash_to_h256(
                        commit.signed_header.header.last_block_id.unwrap().hash,
                    ),
                    part_set_header: PartSetHeader {
                        total: commit
                            .signed_header
                            .header
                            .last_block_id
                            .unwrap()
                            .part_set_header
                            .total,
                        hash: tendermint_hash_to_h256(
                            commit
                                .signed_header
                                .header
                                .last_block_id
                                .unwrap()
                                .part_set_header
                                .hash,
                        ),
                    },
                },
                last_commit_hash: tendermint_hash_to_h256(
                    commit.signed_header.header.last_commit_hash.unwrap(),
                ),
                data_hash: tendermint_hash_to_h256(commit.signed_header.header.data_hash.unwrap()),
                validators_hash: tendermint_hash_to_h256(
                    commit.signed_header.header.validators_hash,
                ),
                next_validators_hash: tendermint_hash_to_h256(
                    commit.signed_header.header.next_validators_hash,
                ),
                consensus_hash: tendermint_hash_to_h256(commit.signed_header.header.consensus_hash),
                app_hash: commit
                    .signed_header
                    .header
                    .app_hash
                    .as_bytes()
                    .try_into()
                    .unwrap(),
                last_results_hash: tendermint_hash_to_h256(
                    commit.signed_header.header.last_results_hash.unwrap(),
                ),
                evidence_hash: tendermint_hash_to_h256(
                    commit.signed_header.header.evidence_hash.unwrap(),
                ),
                proposer_address: commit
                    .signed_header
                    .header
                    .proposer_address
                    .as_bytes()
                    .try_into()
                    .expect("value is a [u8; 20] internally, this should not fail; qed;"),
            },
            commit: Commit {
                height: tendermint_height_to_bounded_i64(commit.signed_header.commit.height),
                round: i32::from(commit.signed_header.commit.round)
                    .try_into()
                    .unwrap(),
                block_id: BlockId {
                    hash: tendermint_hash_to_h256(commit.signed_header.commit.block_id.hash),
                    part_set_header: PartSetHeader {
                        total: commit.signed_header.commit.block_id.part_set_header.total,
                        hash: tendermint_hash_to_h256(
                            commit.signed_header.commit.block_id.part_set_header.hash,
                        ),
                    },
                },
                signatures: commit
                    .signed_header
                    .commit
                    .signatures
                    .into_iter()
                    .map(tendermint_commit_sig_to_commit_sig)
                    .collect(),
            },
        }
    }

    fn tendermint_commit_sig_to_commit_sig(sig: tendermint::block::CommitSig) -> CommitSig {
        match sig {
            ::tendermint::block::CommitSig::BlockIdFlagAbsent => CommitSig::Absent,
            ::tendermint::block::CommitSig::BlockIdFlagCommit {
                validator_address,
                timestamp,
                signature,
            } => CommitSig::Commit {
                validator_address: Vec::from(validator_address).try_into().unwrap(),
                timestamp: {
                    let ts = tendermint_proto::google::protobuf::Timestamp::from(timestamp);

                    Timestamp {
                        seconds: ts.seconds.try_into().unwrap(),
                        nanos: ts.nanos.try_into().unwrap(),
                    }
                },
                signature: signature.unwrap().into_bytes().try_into().unwrap(),
            },
            ::tendermint::block::CommitSig::BlockIdFlagNil {
                validator_address,
                timestamp,
                signature,
            } => CommitSig::Nil {
                validator_address: Vec::from(validator_address).try_into().unwrap(),
                timestamp: {
                    let ts = tendermint_proto::google::protobuf::Timestamp::from(timestamp);

                    Timestamp {
                        seconds: ts.seconds.try_into().unwrap(),
                        nanos: ts.nanos.try_into().unwrap(),
                    }
                },
                signature: signature.unwrap().into_bytes().try_into().unwrap(),
            },
        }
    }

    pub fn tendermint_validator_info_to_validator(val: ::tendermint::validator::Info) -> Validator {
        Validator {
            address: val
                .address
                .as_bytes()
                .try_into()
                .expect("value is 20 bytes internally; should not fail; qed"),
            pub_key: match val.pub_key {
                ::tendermint::PublicKey::Ed25519(key) => PublicKey::Ed25519(key.as_bytes().into()),
                ::tendermint::PublicKey::Bn254(key) => PublicKey::Bn254(key.to_vec()),
                _ => todo!(),
            },
            voting_power: BoundedI64::new(val.power.value().try_into().unwrap()).unwrap(),
            proposer_priority: val.proposer_priority.value(),
        }
    }

    fn tendermint_hash_to_h256(hash: tendermint::Hash) -> H256 {
        match hash {
            tendermint::Hash::Sha256(hash) => hash.into(),
            tendermint::Hash::None => panic!("empty hash???"),
        }
    }

    pub fn tendermint_height_to_bounded_i64(
        height: ::tendermint::block::Height,
    ) -> BoundedI64<0, { i64::MAX }> {
        i64::from(height).try_into().unwrap()
    }
}

pub mod wasm {
    use chain_utils::{
        cosmos::Cosmos,
        cosmos_sdk::{BroadcastTxCommitError, CosmosSdkChain},
        union::Union,
        wasm::Wasm,
    };
    use queue_msg::QueueMsg;
    use serde::{Deserialize, Serialize};
    use unionlabs::{
        encoding::{Encode, Proto},
        google::protobuf::any::{Any, IntoAny},
        hash::H256,
        ibc::lightclients::wasm,
        traits::{ClientState, ClientStateOf, ConsensusStateOf, HeaderOf},
        TypeUrl,
    };

    use crate::{
        chain_impls::{
            cosmos::{CosmosAggregateMsg, CosmosDataMsg, CosmosFetch},
            cosmos_sdk::{
                data::{TrustedCommit, TrustedValidators, UntrustedCommit, UntrustedValidators},
                do_msg, CosmosSdkChainSealed,
            },
            union::{ProveResponse, UnionAggregateMsg, UnionDataMsg, UnionFetch},
        },
        fetch::FetchUpdateHeaders,
        msg::Msg,
        ChainExt, DoFetchUpdateHeaders, DoMsg, RelayerMsgTypes,
    };

    impl ChainExt for Wasm<Union> {
        type Data<Tr: ChainExt> = UnionDataMsg<Wasm<Union>, Tr>;
        type Fetch<Tr: ChainExt> = UnionFetch<Wasm<Union>, Tr>;
        type Aggregate<Tr: ChainExt> = UnionAggregateMsg<Wasm<Union>, Tr>;

        type MsgError = BroadcastTxCommitError;

        type Config = WasmConfig;
    }

    const _: () = {
        try_from_relayer_msg! {
            chain = Wasm<Union>,
            generics = (Tr: ChainExt),
            msgs = UnionDataMsg(
                UntrustedCommit(UntrustedCommit<Wasm<Union>, Tr>),
                TrustedValidators(TrustedValidators<Wasm<Union>, Tr>),
                UntrustedValidators(UntrustedValidators<Wasm<Union>, Tr>),
                ProveResponse(ProveResponse<Wasm<Union>, Tr>),
            ),
        }
    };

    impl ChainExt for Wasm<Cosmos> {
        type Data<Tr: ChainExt> = CosmosDataMsg<Wasm<Cosmos>, Tr>;
        type Fetch<Tr: ChainExt> = CosmosFetch<Wasm<Cosmos>, Tr>;
        type Aggregate<Tr: ChainExt> = CosmosAggregateMsg<Wasm<Cosmos>, Tr>;

        type MsgError = BroadcastTxCommitError;

        type Config = WasmConfig;
    }

    const _: () = {
        try_from_relayer_msg! {
            chain = Wasm<Cosmos>,
            generics = (Tr: ChainExt),
            msgs = CosmosDataMsg(
                TrustedCommit(TrustedCommit<Wasm<Cosmos>, Tr>),
                UntrustedCommit(UntrustedCommit<Wasm<Cosmos>, Tr>),
                TrustedValidators(TrustedValidators<Wasm<Cosmos>, Tr>),
                UntrustedValidators(UntrustedValidators<Wasm<Cosmos>, Tr>),
            ),
        }
    };

    impl<Hc> CosmosSdkChainSealed for Wasm<Hc>
    where
        Wasm<Hc>: ChainExt,
        Hc: CosmosSdkChainSealed,
    {
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    #[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
    pub struct WasmConfig {
        pub checksum: H256,
        // pub inner: T,
    }

    impl<Hc, Tr> DoFetchUpdateHeaders<Self, Tr> for Wasm<Hc>
    where
        Wasm<Hc>: ChainExt,
        Hc: ChainExt + CosmosSdkChain + DoFetchUpdateHeaders<Self, Tr>,
        Tr: ChainExt,
    {
        fn fetch_update_headers(
            hc: &Self,
            update_info: FetchUpdateHeaders<Self, Tr>,
        ) -> QueueMsg<RelayerMsgTypes> {
            Hc::fetch_update_headers(
                hc,
                FetchUpdateHeaders {
                    counterparty_chain_id: update_info.counterparty_chain_id,
                    counterparty_client_id: update_info.counterparty_client_id,
                    update_from: update_info.update_from,
                    update_to: update_info.update_to,
                },
            )
        }
    }

    impl<Hc, Tr> DoMsg<Wasm<Hc>, Tr> for Wasm<Hc>
    where
        Wasm<Hc>: ChainExt<MsgError = BroadcastTxCommitError, Config = WasmConfig>,
        Hc: CosmosSdkChainSealed<MsgError = BroadcastTxCommitError>,
        Tr: ChainExt,

        ConsensusStateOf<Tr>: Encode<Proto> + TypeUrl,
        ClientStateOf<Tr>: Encode<Proto> + TypeUrl,
        HeaderOf<Tr>: Encode<Proto> + TypeUrl,

        ConsensusStateOf<Wasm<Hc>>: Encode<Proto> + TypeUrl,
        ClientStateOf<Wasm<Hc>>: Encode<Proto> + TypeUrl,

        Tr::StoredClientState<Wasm<Hc>>: IntoAny,
        Tr::StateProof: Encode<Proto>,
    {
        async fn msg(&self, msg: Msg<Wasm<Hc>, Tr>) -> Result<(), Self::MsgError> {
            do_msg(
                self,
                msg,
                |config, client_state, consensus_state| {
                    (
                        Any(wasm::client_state::ClientState {
                            latest_height: client_state.height().into(),
                            data: client_state,
                            checksum: config.checksum,
                        })
                        .into(),
                        Any(wasm::consensus_state::ConsensusState {
                            data: consensus_state,
                        })
                        .into(),
                    )
                },
                |client_message| {
                    Any(wasm::client_message::ClientMessage {
                        data: client_message,
                    })
                    .into()
                },
            )
            .await
        }
    }
}
