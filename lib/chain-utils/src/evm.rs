use std::{fmt::Debug, future::Future, marker::PhantomData, num::NonZeroU64, ops::Div, sync::Arc};

use beacon_api::client::BeaconApiClient;
use contracts::{
    devnet_ownable_ibc_handler::{DevnetOwnableIBCHandler, OwnershipTransferredFilter},
    ibc_channel_handshake::{IBCChannelHandshakeErrors, IBCChannelHandshakeEvents},
    ibc_client::{IBCClientErrors, IBCClientEvents},
    ibc_connection::{IBCConnectionErrors, IBCConnectionEvents},
    ibc_handler::{
        GetChannelCall, GetChannelReturn, GetClientStateCall, GetClientStateReturn,
        GetConnectionCall, GetConnectionReturn, GetConsensusStateCall, GetConsensusStateReturn,
        GetHashedPacketAcknowledgementCommitmentCall,
        GetHashedPacketAcknowledgementCommitmentReturn, GetHashedPacketCommitmentCall,
        GetHashedPacketCommitmentReturn, IBCHandler,
    },
    ibc_packet::{IBCPacketErrors, IBCPacketEvents, WriteAcknowledgementFilter},
    shared_types::{IbcCoreChannelV1ChannelData, IbcCoreConnectionV1ConnectionEndData},
};
use ethers::{
    abi::{AbiDecode, Tokenizable},
    contract::{ContractError, EthCall, EthLogDecode},
    core::k256::ecdsa,
    middleware::{NonceManagerMiddleware, SignerMiddleware},
    providers::{Middleware, Provider, ProviderError, Ws, WsClientError},
    signers::{LocalWallet, Wallet},
    utils::secret_key_to_address,
};
use frame_support_procedural::{CloneNoBound, DebugNoBound};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use typenum::Unsigned;
use unionlabs::{
    self,
    encoding::{Decode, EthAbi},
    ethereum::config::ChainSpec,
    hash::{H160, H256},
    ibc::{
        core::client::height::{Height, IsHeight},
        lightclients::{ethereum, tendermint::fraction::Fraction},
    },
    id::{ChannelId, ClientId, PortId},
    option_unwrap, promote,
    proof::{
        AcknowledgementPath, ChannelEndPath, ClientConsensusStatePath, ClientStatePath,
        CommitmentPath, ConnectionPath, IbcPath,
    },
    traits::{
        Chain, ClientIdOf, ClientState, ClientStateOf, ConsensusStateOf, FromStrExact, HeightOf,
        IbcStateEncodingOf,
    },
    uint::U256,
};

use crate::{private_key::PrivateKey, Pool};

pub type EvmSignerMiddleware =
    SignerMiddleware<NonceManagerMiddleware<Provider<Ws>>, Wallet<ecdsa::SigningKey>>;

// TODO(benluelo): Generic over middleware?
#[derive(DebugNoBound, CloneNoBound)]
pub struct Ethereum<C: ChainSpec, S: EvmSignersConfig = ReadWrite> {
    pub chain_id: U256,

    pub ibc_handlers: S::Out,
    pub provider: Arc<Provider<Ws>>,
    pub beacon_api_client: BeaconApiClient<C>,
    /// The address of the `IBCHandler` smart contract.
    pub ibc_handler_address: H160,
}

pub trait EthereumChain: Chain<IbcStateEncoding = EthAbi> {}
impl<C: ChainSpec> EthereumChain for Ethereum<C> {}

#[derive(DebugNoBound, CloneNoBound, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""))]
pub struct Config<S: EvmSignersConfig = ReadWrite> {
    /// The address of the `IBCHandler` smart contract.
    pub ibc_handler_address: H160,

    /// The signer that will be used to submit transactions by voyager.
    pub signers: S::Config,

    /// The RPC endpoint for the execution chain.
    pub eth_rpc_api: String,
    /// The RPC endpoint for the beacon chain.
    pub eth_beacon_rpc_api: String,
}

pub trait EvmSignersConfig: Send + Sync + 'static {
    type Config: Debug + Clone + Serialize + DeserializeOwned + Send + Sync + 'static;
    type Out: Debug + Clone + Send + Sync + 'static;

    fn new(
        config: Self::Config,
        ibc_handler_address: H160,
        chain_id: u64,
        provider: Provider<Ws>,
    ) -> Self::Out;
}

pub enum Readonly {}

impl EvmSignersConfig for Readonly {
    type Config = ();
    type Out = ();

    fn new(
        config: Self::Config,
        ibc_handler_address: H160,
        chain_id: u64,
        provider: Provider<Ws>,
    ) -> Self::Out {
        config
    }
}

pub enum ReadWrite {}

impl EvmSignersConfig for ReadWrite {
    type Config = Vec<PrivateKey<ecdsa::SigningKey>>;
    type Out = Pool<IBCHandler<EvmSignerMiddleware>>;

    fn new(
        config: Self::Config,
        ibc_handler_address: H160,
        chain_id: u64,
        provider: Provider<Ws>,
    ) -> Self::Out {
        Pool::new(config.into_iter().map(|signer| {
            let signing_key: ecdsa::SigningKey = signer.value();
            let address = secret_key_to_address(&signing_key);

            let wallet = LocalWallet::new_with_signer(signing_key, address, chain_id);

            let signer_middleware = Arc::new(SignerMiddleware::new(
                NonceManagerMiddleware::new(provider.clone(), address),
                wallet.clone(),
            ));

            IBCHandler::new(ibc_handler_address.clone(), signer_middleware.clone())
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EvmChainType<C: ChainSpec>(PhantomData<fn() -> C>);

pub const EVM_REVISION_NUMBER: u64 = 0;

impl<C: ChainSpec> FromStrExact for EvmChainType<C> {
    const EXPECTING: &'static str = {
        const PREFIX: [u8; 4] = *b"eth-";

        const fn concat(cs: &[u8]) -> [u8; 11] {
            [
                PREFIX[0], PREFIX[1], PREFIX[2], PREFIX[3], cs[0], cs[1], cs[2], cs[3], cs[4],
                cs[5], cs[6],
            ]
        }

        // generic_const_exprs is still incomplete :(
        const CHAIN_SPEC_LEN: usize = 7;
        assert!(
            C::EXPECTING.len() == CHAIN_SPEC_LEN,
            "ChainSpec string value is expected to be 7 bytes"
        );

        match core::str::from_utf8(&concat(C::EXPECTING.as_bytes())) {
            Ok(ok) => ok,
            Err(_) => {
                panic!()
            }
        }
    };
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum IBCHandlerEvents {
    PacketEvent(IBCPacketEvents),
    ConnectionEvent(IBCConnectionEvents),
    ChannelEvent(IBCChannelHandshakeEvents),
    ClientEvent(IBCClientEvents),
    OwnableEvent(OwnershipTransferredFilter),
}

impl EthLogDecode for IBCHandlerEvents {
    fn decode_log(log: &ethers::abi::RawLog) -> Result<Self, ethers::abi::Error>
    where
        Self: Sized,
    {
        macro_rules! try_decode {
            ($($expr:expr),+) => {
                $(
                    if let Ok(ok) = $expr {
                        return Ok(ok);
                    }
                )+
            };
        }

        try_decode! {
            IBCPacketEvents::decode_log(log).map(IBCHandlerEvents::PacketEvent),
            IBCConnectionEvents::decode_log(log).map(IBCHandlerEvents::ConnectionEvent),
            IBCChannelHandshakeEvents::decode_log(log).map(IBCHandlerEvents::ChannelEvent),
            IBCClientEvents::decode_log(log).map(IBCHandlerEvents::ClientEvent),
            OwnershipTransferredFilter::decode_log(log).map(IBCHandlerEvents::OwnableEvent)
        }

        Err(ethers::abi::Error::InvalidData)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum IbcHandlerErrors {
    PacketEvent(IBCPacketErrors),
    ConnectionEvent(IBCConnectionErrors),
    ChannelEvent(IBCChannelHandshakeErrors),
    ClientEvent(IBCClientErrors),
}

impl AbiDecode for IbcHandlerErrors {
    fn decode(bytes: impl AsRef<[u8]>) -> Result<Self, ethers::abi::AbiError> {
        let packet_error = IBCPacketErrors::decode(&bytes).map(IbcHandlerErrors::PacketEvent);
        let conn_error = IBCConnectionErrors::decode(&bytes).map(IbcHandlerErrors::ConnectionEvent);
        let chan_error =
            IBCChannelHandshakeErrors::decode(&bytes).map(IbcHandlerErrors::ChannelEvent);
        let client_error = IBCClientErrors::decode(&bytes).map(IbcHandlerErrors::ClientEvent);
        [packet_error, conn_error, chan_error, client_error]
            .into_iter()
            .find(|error| error.is_ok())
            .ok_or(ethers::abi::Error::InvalidData)?
    }
}

impl<C: ChainSpec, S: EvmSignersConfig> Chain for Ethereum<C, S> {
    type ChainType = EvmChainType<C>;

    type SelfClientState = ethereum::client_state::ClientState;
    type SelfConsensusState = ethereum::consensus_state::ConsensusState;

    type StoredClientState<Tr: Chain> = Tr::SelfClientState;
    type StoredConsensusState<Tr: Chain> = Tr::SelfConsensusState;

    type Header = ethereum::header::Header<C>;

    type Height = Height;

    type ClientId = ClientId;

    type IbcStateEncoding = EthAbi;

    type ClientType = String;

    type Error = beacon_api::errors::Error;

    type StateProof = unionlabs::ibc::lightclients::ethereum::storage_proof::StorageProof;

    fn chain_id(&self) -> <Self::SelfClientState as ClientState>::ChainId {
        self.chain_id
    }

    async fn query_latest_height(&self) -> Result<Height, Self::Error> {
        self.beacon_api_client
            .finality_update()
            .await
            .map(|response| self.make_height(response.data.attested_header.beacon.slot))
    }

    async fn query_latest_height_as_destination(&self) -> Result<Height, Self::Error> {
        let height = self
            .beacon_api_client
            .block(beacon_api::client::BlockId::Head)
            .await?
            .data
            .message
            .slot;

        // HACK: we introduced this because we were using alchemy for the
        // execution endpoint and our custom beacon endpoint that rely on
        // its own execution chain. Alchemy was a bit delayed and the
        // execution height for the beacon head wasn't existing for few
        // secs. We wait for an extra beacon head to let alchemy catch up.
        // We should be able to remove that once we rely on an execution
        // endpoint that is itself used by the beacon endpoint (no different
        // POV).
        loop {
            let next_height = self
                .beacon_api_client
                .block(beacon_api::client::BlockId::Head)
                .await?
                .data
                .message
                .slot;
            if next_height > height {
                break;
            }

            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }

        Ok(self.make_height(height))
    }

    async fn query_latest_timestamp(&self) -> Result<i64, Self::Error> {
        Ok(self
            .beacon_api_client
            .finality_update()
            .await?
            .data
            .attested_header
            .execution
            .timestamp
            .try_into()
            .unwrap())
    }

    async fn self_client_state(&self, beacon_height: Height) -> Self::SelfClientState {
        let genesis = self.beacon_api_client.genesis().await.unwrap().data;

        ethereum::client_state::ClientState {
            chain_id: self.chain_id,
            genesis_validators_root: genesis.genesis_validators_root,
            genesis_time: genesis.genesis_time,
            fork_parameters: self
                .beacon_api_client
                .spec()
                .await
                .unwrap()
                .data
                .into_fork_parameters(),
            // REVIEW: Is this a preset config param? Or a per-chain config?
            seconds_per_slot: C::SECONDS_PER_SLOT::U64,
            slots_per_epoch: C::SLOTS_PER_EPOCH::U64,
            epochs_per_sync_committee_period: C::EPOCHS_PER_SYNC_COMMITTEE_PERIOD::U64,
            trusting_period: 100_000_000,
            latest_slot: beacon_height.revision_height,
            min_sync_committee_participants: 0,
            trust_level: Fraction {
                numerator: 1,
                denominator: promote!(NonZeroU64: option_unwrap!(NonZeroU64::new(3))),
            },
            frozen_height: Height {
                revision_number: 0,
                revision_height: 0,
            },
            counterparty_commitment_slot: U256::from(0),
            ibc_contract_address: self.ibc_handler_address.clone(),
        }
    }

    async fn self_consensus_state(&self, beacon_height: Height) -> Self::SelfConsensusState {
        let trusted_header = self
            .beacon_api_client
            .header(beacon_api::client::BlockId::Slot(
                beacon_height.revision_height,
            ))
            .await
            .unwrap()
            .data;

        let bootstrap = self
            .beacon_api_client
            .bootstrap(trusted_header.root)
            .await
            .unwrap()
            .data;

        assert!(bootstrap.header.beacon.slot == beacon_height.revision_height);

        let light_client_update = {
            let current_period = beacon_height.revision_height.div(C::PERIOD::U64);

            tracing::info!(%current_period);

            let light_client_updates = self
                .beacon_api_client
                .light_client_updates(current_period, 1)
                .await
                .unwrap();

            let [light_client_update] = &*light_client_updates.0 else {
                panic!()
            };

            light_client_update.data.clone()
        };

        let timestamp = bootstrap.header.execution.timestamp;
        ethereum::consensus_state::ConsensusState {
            slot: bootstrap.header.beacon.slot,
            state_root: bootstrap.header.execution.state_root,
            // TODO: Should this shouldn't be the default but fetched via eth_getProof
            storage_root: H256::default(),
            timestamp,
            current_sync_committee: bootstrap.current_sync_committee.aggregate_pubkey,
            next_sync_committee: light_client_update
                .next_sync_committee
                .map(|nsc| nsc.aggregate_pubkey),
        }
    }

    async fn read_ack(
        &self,
        tx_hash: H256,
        destination_channel_id: ChannelId,
        destination_port_id: PortId,
        sequence: NonZeroU64,
    ) -> Vec<u8> {
        evm_read_ack(
            &self.provider,
            tx_hash,
            destination_port_id,
            destination_channel_id,
            sequence,
        )
        .await
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EvmInitError {
    #[error("unable to connect to websocket")]
    Ws(#[from] WsClientError),
    #[error("provider error")]
    Provider(#[from] ProviderError),
}

impl<C: ChainSpec, S: EvmSignersConfig> Ethereum<C, S> {
    pub async fn new(config: Config<S>) -> Result<Self, EvmInitError> {
        let provider = Provider::new(Ws::connect(config.eth_rpc_api).await?);

        let chain_id = provider.get_chainid().await?;

        Ok(Self {
            chain_id: U256(chain_id),
            ibc_handlers: S::new(
                config.signers,
                config.ibc_handler_address.clone(),
                chain_id.as_u64(),
                provider.clone(),
            ),
            ibc_handler_address: config.ibc_handler_address,
            provider: Arc::new(provider),
            beacon_api_client: BeaconApiClient::new(config.eth_beacon_rpc_api).await,
        })
    }
}

async fn evm_read_ack(
    provider: &Provider<Ws>,
    tx_hash: H256,
    destination_port_id: PortId,
    destination_channel_id: ChannelId,
    sequence: NonZeroU64,
) -> Vec<u8> {
    provider
        .get_transaction_receipt(tx_hash.clone())
        .await
        .unwrap()
        .unwrap()
        .logs
        .into_iter()
        .map(|log| <WriteAcknowledgementFilter as EthLogDecode>::decode_log(&log.into()))
        .find_map(|e| match e {
            Ok(WriteAcknowledgementFilter {
                destination_port: ack_dst_port_id,
                destination_channel,
                sequence: ack_sequence,
                acknowledgement,
            }) if ack_dst_port_id == destination_port_id.to_string()
                && destination_channel == destination_channel_id.to_string()
                && Some(sequence) == NonZeroU64::new(ack_sequence) =>
            {
                Some(acknowledgement)
            }
            _ => None,
        })
        .unwrap_or_default()
        .to_vec()
}

impl<C: ChainSpec, S: EvmSignersConfig> Ethereum<C, S> {
    pub fn make_height(&self, height: impl Into<u64>) -> Height {
        // NOTE: Revision is always 0 for EVM
        // REVIEW: Consider using the fork revision?
        {
            Height {
                revision_number: EVM_REVISION_NUMBER,
                revision_height: height.into(),
            }
        }
    }
}

pub trait HasIbcHandler {
    fn ibc_handler(&self) -> IBCHandler<Provider<Ws>>;
}

impl<C: ChainSpec, S: EvmSignersConfig> HasIbcHandler for Ethereum<C, S> {
    fn ibc_handler(&self) -> IBCHandler<Provider<Ws>> {
        IBCHandler::new(self.ibc_handler_address.clone(), self.provider.clone())
    }
}

pub trait IbcHandlerExt<M: Middleware> {
    fn ibc_state_read<P, Hc, Tr>(
        &self,
        execution_block_number: u64,
        path: P,
    ) -> impl Future<Output = Result<P::Output, ContractError<M>>> + Send
    where
        P: EthereumStateRead<Hc, Tr> + 'static,
        Hc: EthereumChain,
        Tr: Chain;

    fn eth_call<Call>(
        &self,
        call: Call,
        at_execution_height: u64,
    ) -> impl Future<
        Output = Result<
            Option<<<Call as EthCallExt>::Return as TupleToOption>::Inner>,
            ContractError<M>,
        >,
    > + Send
    where
        Call: EthCallExt + 'static,
        Call::Return: TupleToOption;
}

impl<M: Middleware> IbcHandlerExt<M> for IBCHandler<M> {
    async fn ibc_state_read<P, Hc, Tr>(
        &self,
        execution_block_number: u64,
        path: P,
    ) -> Result<P::Output, ContractError<M>>
    where
        P: EthereumStateRead<Hc, Tr> + 'static,
        Hc: EthereumChain,
        Tr: Chain,
    {
        self.method_hash::<P::EthCall, <P::EthCall as EthCallExt>::Return>(
            P::EthCall::selector(),
            path.into_eth_call(),
        )
        .expect("valid contract selector")
        .block(execution_block_number)
        .call()
        .await
        .map(P::decode_ibc_state)
    }

    async fn eth_call<Call>(
        &self,
        call: Call,
        at_execution_height: u64,
    ) -> Result<Option<<<Call as EthCallExt>::Return as TupleToOption>::Inner>, ContractError<M>>
    where
        Call: EthCallExt + 'static,
        Call::Return: TupleToOption,
    {
        self.method_hash::<Call, Call::Return>(Call::selector(), call)
            .expect("valid contract selector")
            .block(at_execution_height)
            .call()
            .await
            .map(Call::Return::tuple_to_option)
    }
}

/// Many contract calls return some form of [`(bool, T)`] as a way to emulate nullable/[`Option`].
/// This trait allows for easy conversion from the aforementioned tuple to an [`Option`].
pub trait TupleToOption {
    type Inner;

    fn tuple_to_option(self) -> Option<Self::Inner>;
}

impl TupleToOption for GetClientStateReturn {
    type Inner = Vec<u8>;

    fn tuple_to_option(self) -> Option<Self::Inner> {
        self.1.then_some(self.0.to_vec())
    }
}

impl TupleToOption for GetConsensusStateReturn {
    type Inner = Vec<u8>;

    fn tuple_to_option(self) -> Option<Self::Inner> {
        self.p1.then_some(self.consensus_state_bytes.to_vec())
    }
}

impl TupleToOption for GetConnectionReturn {
    type Inner = IbcCoreConnectionV1ConnectionEndData;

    fn tuple_to_option(self) -> Option<Self::Inner> {
        self.1.then_some(self.0)
    }
}

impl TupleToOption for GetChannelReturn {
    type Inner = IbcCoreChannelV1ChannelData;

    fn tuple_to_option(self) -> Option<Self::Inner> {
        self.1.then_some(self.0)
    }
}

impl TupleToOption for GetHashedPacketCommitmentReturn {
    type Inner = [u8; 32];

    fn tuple_to_option(self) -> Option<Self::Inner> {
        self.1.then_some(self.0)
    }
}

impl TupleToOption for GetHashedPacketAcknowledgementCommitmentReturn {
    type Inner = [u8; 32];

    fn tuple_to_option(self) -> Option<Self::Inner> {
        self.1.then_some(self.0)
    }
}

/// Wrapper trait for a contract call's signature, to map the input type to the return type.
/// `ethers` generates both of these types, but doesn't correlate them.
pub trait EthCallExt: EthCall {
    type Return: Send + Sync + Tokenizable;
}

macro_rules! impl_eth_call_ext {
    ($($Call:ident -> $Return:ident;)+) => {
        $(
            impl EthCallExt for $Call {
                type Return = $Return;
            }
        )+
    }
}

impl_eth_call_ext! {
    GetClientStateCall                           -> GetClientStateReturn;
    GetConsensusStateCall                        -> GetConsensusStateReturn;
    GetConnectionCall                            -> GetConnectionReturn;
    GetChannelCall                               -> GetChannelReturn;
    GetHashedPacketCommitmentCall                -> GetHashedPacketCommitmentReturn;
    GetHashedPacketAcknowledgementCommitmentCall -> GetHashedPacketAcknowledgementCommitmentReturn;
}

pub trait EthereumStateRead<Hc, Tr>: IbcPath<Hc, Tr>
where
    Hc: EthereumChain,
    Tr: Chain,
{
    type EthCall: EthCallExt + 'static;

    fn into_eth_call(self) -> Self::EthCall;

    fn decode_ibc_state(encoded: <Self::EthCall as EthCallExt>::Return) -> Self::Output;
}

impl<Hc, Tr> EthereumStateRead<Hc, Tr> for ClientStatePath<ClientIdOf<Hc>>
where
    Hc: EthereumChain,
    Tr: Chain,
    ClientStateOf<Tr>: Decode<IbcStateEncodingOf<Hc>>,
    Tr::SelfClientState: Decode<EthAbi> + unionlabs::EthAbi,
    <Tr::SelfClientState as unionlabs::EthAbi>::EthAbi: From<Tr::SelfClientState>,
    <Hc as Chain>::StoredClientState<Tr>: unionlabs::EthAbi,
    <Hc as Chain>::StoredClientState<Tr>:
        From<<<Hc as Chain>::StoredClientState<Tr> as unionlabs::EthAbi>::EthAbi>,
{
    type EthCall = GetClientStateCall;

    fn into_eth_call(self) -> Self::EthCall {
        Self::EthCall {
            client_id: self.client_id.to_string(),
        }
    }

    fn decode_ibc_state(encoded: <Self::EthCall as EthCallExt>::Return) -> Self::Output {
        <Self::Output as Decode<EthAbi>>::decode(&encoded.0).unwrap()
    }
}

impl<Hc, Tr> EthereumStateRead<Hc, Tr> for ClientConsensusStatePath<ClientIdOf<Hc>, HeightOf<Tr>>
where
    Hc: EthereumChain,
    Tr: Chain,
    ConsensusStateOf<Tr>: Decode<EthAbi>,
    Tr::SelfClientState: Decode<EthAbi> + unionlabs::EthAbi,
    <Tr::SelfClientState as unionlabs::EthAbi>::EthAbi: From<Tr::SelfClientState>,
    <Hc as Chain>::StoredConsensusState<Tr>: unionlabs::EthAbi,
    <Hc as Chain>::StoredConsensusState<Tr>:
        From<<<Hc as Chain>::StoredConsensusState<Tr> as unionlabs::EthAbi>::EthAbi>,
{
    type EthCall = GetConsensusStateCall;

    fn into_eth_call(self) -> Self::EthCall {
        Self::EthCall {
            client_id: self.client_id.to_string(),
            height: self.height.into_height().into(),
        }
    }

    fn decode_ibc_state(encoded: <Self::EthCall as EthCallExt>::Return) -> Self::Output {
        <Self::Output as Decode<EthAbi>>::decode(&encoded.consensus_state_bytes).unwrap()
    }
}

impl<Hc: EthereumChain, Tr: Chain> EthereumStateRead<Hc, Tr> for ConnectionPath {
    type EthCall = GetConnectionCall;

    fn into_eth_call(self) -> Self::EthCall {
        Self::EthCall {
            connection_id: self.connection_id.to_string(),
        }
    }

    fn decode_ibc_state(encoded: <Self::EthCall as EthCallExt>::Return) -> Self::Output {
        encoded.0.try_into().unwrap()
    }
}

impl<Hc: EthereumChain, Tr: Chain> EthereumStateRead<Hc, Tr> for ChannelEndPath {
    type EthCall = GetChannelCall;

    fn into_eth_call(self) -> Self::EthCall {
        Self::EthCall {
            port_id: self.port_id.to_string(),
            channel_id: self.channel_id.to_string(),
        }
    }

    fn decode_ibc_state(encoded: <Self::EthCall as EthCallExt>::Return) -> Self::Output {
        encoded.0.try_into().unwrap()
    }
}

impl<Hc: EthereumChain, Tr: Chain> EthereumStateRead<Hc, Tr> for CommitmentPath {
    type EthCall = GetHashedPacketCommitmentCall;

    fn into_eth_call(self) -> Self::EthCall {
        Self::EthCall {
            port_id: self.port_id.to_string(),
            channel_id: self.channel_id.to_string(),
            sequence: self.sequence.get(),
        }
    }

    fn decode_ibc_state(encoded: <Self::EthCall as EthCallExt>::Return) -> Self::Output {
        encoded.0.into()
    }
}

impl<Hc: EthereumChain, Tr: Chain> EthereumStateRead<Hc, Tr> for AcknowledgementPath {
    type EthCall = GetHashedPacketAcknowledgementCommitmentCall;

    fn into_eth_call(self) -> Self::EthCall {
        Self::EthCall {
            port_id: self.port_id.to_string(),
            channel_id: self.channel_id.to_string(),
            sequence: self.sequence.get(),
        }
    }

    fn decode_ibc_state(encoded: <Self::EthCall as EthCallExt>::Return) -> Self::Output {
        encoded.0.into()
    }
}

#[allow(unused_variables)]
pub async fn setup_initial_channel<C: ChainSpec>(
    this: &Ethereum<C, ReadWrite>,
    module_address: H160,
    channel_id: String,
    port_id: String,
    counterparty_port_id: String,
) {
    // let signer_middleware = Arc::new(SignerMiddleware::new(
    //     this.provider.clone(),
    //     this.wallet.clone(),
    // ));

    // let ibc_handler = devnet_ownable_ibc_handler::DevnetOwnableIBCHandler::new(
    //     this.ibc_handler.address(),
    //     signer_middleware,
    // );

    // ibc_handler
    //     .setup_initial_channel(
    //         "connection-0".into(),
    //         IbcCoreConnectionV1ConnectionEndData {
    //             client_id: "cometbls-new-0".into(),
    //             versions: vec![IbcCoreConnectionV1VersionData {
    //                 identifier: "1".into(),
    //                 features: vec!["ORDER_ORDERED".into(), "ORDER_UNORDERED".into()],
    //             }],
    //             state: 3,
    //             counterparty: IbcCoreConnectionV1TrData {
    //                 client_id: "08-wasm-0".into(),
    //                 connection_id: "connection-0".into(),
    //                 prefix: IbcCoreCommitmentV1MerklePrefixData {
    //                     key_prefix: b"ibc".to_vec().into(),
    //                 },
    //             },
    //             delay_period: 6,
    //         },
    //         port_id,
    //         channel_id.clone(),
    //         IbcCoreChannelV1ChannelData {
    //             state: 3,
    //             ordering: 1,
    //             counterparty: IbcCoreChannelV1TrData {
    //                 port_id: counterparty_port_id,
    //                 channel_id,
    //             },
    //             connection_hops: vec!["connection-0".into()],
    //             version: "ics20-1".into(),
    //         },
    //         module_address.into(),
    //     )
    //     .send()
    //     .await
    //     .unwrap()
    //     .await
    //     .unwrap()
    //     .unwrap();
    todo!()
}

#[test]
fn eth_chain_type() {
    assert_eq!(
        <<Ethereum<unionlabs::ethereum::config::Mainnet, Readonly> as Chain>::ChainType as FromStrExact>::EXPECTING,
        "eth-mainnet",
    );
    assert_eq!(
        <<Ethereum<unionlabs::ethereum::config::Minimal, Readonly> as Chain>::ChainType as FromStrExact>::EXPECTING,
        "eth-minimal",
    );
}
