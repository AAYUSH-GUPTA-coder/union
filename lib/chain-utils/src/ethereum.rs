use std::{fmt::Debug, marker::PhantomData, ops::Div, sync::Arc};

use beacon_api::client::BeaconApiClient;
use contracts::{
    cometbls_client::CometblsClientErrors,
    i_light_client::ILightClient,
    ibc_channel_handshake::{IBCChannelHandshakeErrors, IBCChannelHandshakeEvents},
    ibc_client::{IBCClientErrors, IBCClientEvents},
    ibc_connection::{IBCConnectionErrors, IBCConnectionEvents},
    ibc_handler::{IBCHandler, OwnershipTransferredFilter},
    ibc_packet::{IBCPacketErrors, IBCPacketEvents},
};
use ethers::{
    abi::{AbiDecode, AbiEncode},
    contract::{ContractError, EthLogDecode},
    core::k256::ecdsa,
    middleware::{NonceManagerMiddleware, SignerMiddleware},
    providers::{Middleware, Provider, ProviderError, Ws, WsClientError},
    signers::{LocalWallet, Wallet},
    utils::{keccak256, secret_key_to_address},
};
use frame_support_procedural::{CloneNoBound, DebugNoBound};
use futures::Future;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tracing::debug;
use typenum::Unsigned;
use unionlabs::{
    encoding::{Decode, EthAbi},
    ethereum::{config::ChainSpec, IBC_HANDLER_COMMITMENTS_SLOT},
    hash::{H160, H256},
    ibc::{
        core::client::height::{Height, IsHeight},
        lightclients::ethereum::{self, storage_proof::StorageProof},
    },
    ics24::{
        AcknowledgementPath, ChannelEndPath, ClientConsensusStatePath, ClientStatePath,
        CommitmentPath, ConnectionPath, IbcPath, NextClientSequencePath,
        NextConnectionSequencePath, NextSequenceRecvPath, ReceiptPath,
    },
    id::ClientId,
    iter,
    traits::{Chain, ClientIdOf, ClientState, FromStrExact, HeightOf},
    uint::U256,
};

use crate::keyring::{ChainKeyring, ConcurrentKeyring, KeyringConfig, KeyringEntry, SignerBalance};

pub type EthereumKeyring = ConcurrentKeyring<H160, IBCHandler<EthereumSignerMiddleware>>;

pub type EthereumSignerMiddleware =
    SignerMiddleware<NonceManagerMiddleware<Provider<Ws>>, Wallet<ecdsa::SigningKey>>;

// NOTE: ClientType bound is temporary until I figure out a better way to deal with client types
/// A chain running the EVM and our solidity IBC stack. This can be any Ethereum L1 or L2, or a chain running the EVM in a different environment (such as Berachain).
pub trait EthereumExecutionRpcs {
    /// The provider connected to this chain's [JSON-RPC](https://ethereum.org/en/developers/docs/apis/json-rpc/).
    fn provider(&self) -> Arc<Provider<Ws>>;

    /// The address of the [`IBCHandler`] smart contract deployed natively on this chain.
    fn ibc_handler_address(&self) -> H160;
}

pub trait EthereumChain =
    EthereumExecutionRpcs + Chain<IbcStateEncoding = EthAbi, ClientType = String>;

/// An Ethereum-based chain. This can be any chain that is based off of and settles on Ethereum (i.e. Ethereum mainnet/ Sepolia, L2s such as Scroll).
pub trait EthereumConsensusChain: EthereumChain<StateProof = StorageProof> {
    /// Fetch the execution height associated with the given beacon slot. For [`Ethereum`], this will simply be the execution block number, but for L2s this will fetch the settled height at the L1 block number.
    fn execution_height_of_beacon_slot(&self, slot: u64) -> impl Future<Output = u64>;

    // NOTE: This is a stopgap solution until we stop using ethers and write our own eth rpc library
    fn get_proof(
        &self,
        address: H160,
        location: U256,
        block: u64,
    ) -> impl Future<Output = StorageProof>;
}

#[diagnostic::on_unimplemented(message = "{Self} does not implement `EthereumExecutionRpcs`")]
pub trait EthereumExecutionRpcsExt: EthereumExecutionRpcs {
    /// Convenience method to construct an [`IBCHandler`] instance for this chain.
    fn ibc_handler(&self) -> IBCHandler<Provider<Ws>> {
        IBCHandler::new(self.ibc_handler_address(), self.provider())
    }
}

impl<T: EthereumExecutionRpcs> EthereumExecutionRpcsExt for T {}

impl<C: ChainSpec, S: EthereumSignersConfig> EthereumExecutionRpcs for Ethereum<C, S> {
    fn provider(&self) -> Arc<Provider<Ws>> {
        self.provider.clone()
    }

    fn ibc_handler_address(&self) -> H160 {
        self.ibc_handler_address
    }
}

impl<C: ChainSpec, S: EthereumSignersConfig> EthereumConsensusChain for Ethereum<C, S> {
    async fn execution_height_of_beacon_slot(&self, slot: u64) -> u64 {
        self.beacon_api_client
            .execution_height(beacon_api::client::BlockId::Slot(slot))
            .await
            .unwrap()
    }

    async fn get_proof(&self, address: H160, location: U256, block: u64) -> StorageProof {
        get_proof(self, address, location, block).await
    }
}

// TODO(benluelo): Generic over middleware?
#[derive(DebugNoBound, CloneNoBound)]
pub struct Ethereum<C: ChainSpec, S: EthereumSignersConfig = ReadWrite> {
    pub chain_id: U256,

    /// The address of the `IBCHandler` smart contract.
    pub ibc_handler_address: H160,

    pub keyring: S::Out,
    pub provider: Arc<Provider<Ws>>,
    pub beacon_api_client: BeaconApiClient<C>,
}

#[derive(DebugNoBound, CloneNoBound, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""))]
pub struct Config<S: EthereumSignersConfig = ReadWrite> {
    /// The address of the `IBCHandler` smart contract.
    pub ibc_handler_address: H160,

    /// The signer that will be used to submit transactions by voyager.
    // TODO: Write a custom ser/de implementation for this struct to avoid this hackery
    #[serde(skip_serializing_if = "is_unit", default)]
    pub keyring: S::Config,

    /// The RPC endpoint for the execution chain.
    pub eth_rpc_api: String,
    /// The RPC endpoint for the beacon chain.
    pub eth_beacon_rpc_api: String,
}

// lol
fn is_unit<T: 'static>(_: &T) -> bool {
    std::any::TypeId::of::<T>() == std::any::TypeId::of::<()>()
}

impl<C: ChainSpec> ChainKeyring for Ethereum<C, ReadWrite> {
    type Address = H160;

    type Signer = IBCHandler<EthereumSignerMiddleware>;

    fn keyring(&self) -> &ConcurrentKeyring<Self::Address, Self::Signer> {
        &self.keyring
    }

    async fn balances(&self) -> Vec<SignerBalance<Self::Address>> {
        balance_of_signers(&self.keyring, &self.provider).await
    }
}

pub async fn balance_of_signers(
    keyring: &EthereumKeyring,
    provider: &Provider<Ws>,
) -> Vec<SignerBalance<H160>> {
    let mut out_vec = vec![];

    for (key_name, &address) in keyring.keys() {
        out_vec.push(SignerBalance {
            key_name: key_name.to_owned(),
            address,
            balance: provider
                .get_balance(ethers::types::H160::from(address), None)
                .await
                .unwrap()
                .as_u128(),
            denom: "".to_owned(),
        });
    }

    out_vec
}

pub trait EthereumSignersConfig: Send + Sync + 'static {
    type Config: Debug + Clone + Default + Serialize + DeserializeOwned + Send + Sync + 'static;
    type Out: Debug + Clone + Send + Sync + 'static;

    fn new(
        config: Self::Config,
        ibc_handler_address: H160,
        chain_id: u64,
        provider: Provider<Ws>,
    ) -> Self::Out;
}

pub enum Readonly {}

impl EthereumSignersConfig for Readonly {
    type Config = ();
    type Out = ();

    fn new(
        config: Self::Config,
        _ibc_handler_address: H160,
        _chain_id: u64,
        _provider: Provider<Ws>,
    ) -> Self::Out {
        config
    }
}

pub enum ReadWrite {}

impl EthereumSignersConfig for ReadWrite {
    type Config = KeyringConfig;
    type Out = EthereumKeyring;

    fn new(
        config: Self::Config,
        ibc_handler_address: H160,
        chain_id: u64,
        provider: Provider<Ws>,
    ) -> Self::Out {
        ConcurrentKeyring::new(
            config.name,
            config.keys.into_iter().map(|config| {
                let signing_key = <ecdsa::SigningKey as bip32::PrivateKey>::from_bytes(
                    &config.value().as_slice().try_into().unwrap(),
                )
                .unwrap();

                let address = secret_key_to_address(&signing_key);

                let wallet = LocalWallet::new_with_signer(signing_key, address, chain_id);

                let signer_middleware = Arc::new(SignerMiddleware::new(
                    NonceManagerMiddleware::new(provider.clone(), address),
                    wallet.clone(),
                ));

                KeyringEntry {
                    name: config.name(),
                    address: address.into(),
                    signer: IBCHandler::new(ibc_handler_address, signer_middleware.clone()),
                }
            }),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EthereumChainType<C: ChainSpec>(PhantomData<fn() -> C>);

pub const ETHEREUM_REVISION_NUMBER: u64 = 0;

impl<C: ChainSpec> FromStrExact for EthereumChainType<C> {
    const EXPECTING: &'static str = {
        match core::str::from_utf8(
            const {
                let mut buf = [0_u8; 32];

                iter! {
                    for (i, b) in enumerate(b"eth-") {
                        buf[i] = b;
                    }
                }

                iter! {
                    for (i, b) in enumerate(C::EXPECTING.as_bytes()) {
                        buf[4 + i] = b;
                    }
                }

                buf
            }
            .split_at(4 + C::EXPECTING.len())
            .0,
        ) {
            Ok(ok) => ok,
            Err(_) => panic!("called `Result::unwrap()` on an `Err` value"),
        }
    };
}

macro_rules! try_decode {
    ($($expr:expr),+) => {
        $(
            if let Ok(ok) = $expr {
                return Ok(ok);
            }
        )+
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
        try_decode! {
            IBCPacketEvents::decode_log(log).map(IBCHandlerEvents::PacketEvent),
            IBCConnectionEvents::decode_log(log).map(IBCHandlerEvents::ConnectionEvent),
            IBCChannelHandshakeEvents::decode_log(log).map(IBCHandlerEvents::ChannelEvent),
            IBCClientEvents::decode_log(log).map(IBCHandlerEvents::ClientEvent),
            OwnershipTransferredFilter::decode_log(log).map(IBCHandlerEvents::OwnableEvent)
        };
        Err(ethers::abi::Error::InvalidData)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum IbcHandlerErrors {
    PacketErrors(IBCPacketErrors),
    ConnectionErrors(IBCConnectionErrors),
    ChannelErrors(IBCChannelHandshakeErrors),
    ClientErrors(IBCClientErrors),
    CometblsClientErrors(CometblsClientErrors),
}

impl AbiDecode for IbcHandlerErrors {
    fn decode(bytes: impl AsRef<[u8]>) -> Result<Self, ethers::abi::AbiError> {
        try_decode! {
            IBCPacketErrors::decode(&bytes).map(IbcHandlerErrors::PacketErrors),
            IBCConnectionErrors::decode(&bytes).map(IbcHandlerErrors::ConnectionErrors),
            IBCChannelHandshakeErrors::decode(&bytes).map(IbcHandlerErrors::ChannelErrors),
            IBCClientErrors::decode(&bytes).map(IbcHandlerErrors::ClientErrors),
            CometblsClientErrors::decode(&bytes).map(IbcHandlerErrors::CometblsClientErrors)
        };
        Err(ethers::abi::Error::InvalidData.into())
    }
}

impl<C: ChainSpec, S: EthereumSignersConfig> Chain for Ethereum<C, S> {
    type ChainType = EthereumChainType<C>;

    type SelfClientState = ethereum::client_state::ClientState;
    type SelfConsensusState = ethereum::consensus_state::ConsensusState;

    type StoredClientState<Tr: Chain> = Tr::SelfClientState;
    type StoredConsensusState<Tr: Chain> = Tr::SelfConsensusState;

    type Header = ethereum::header::Header<C>;

    type Height = Height;

    type ClientId = EthereumClientId;

    type IbcStateEncoding = EthAbi;

    type ClientType = String;

    type Error = beacon_api::errors::Error;

    type StateProof = StorageProof;

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
            latest_slot: beacon_height.revision_height,
            min_sync_committee_participants: 0,
            frozen_height: Height {
                revision_number: 0,
                revision_height: 0,
            },
            ibc_commitment_slot: IBC_HANDLER_COMMITMENTS_SLOT,
            ibc_contract_address: self.ibc_handler_address,
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

            debug!(%current_period);

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

        // Normalize to nanos in order to be compliant with cosmos
        let timestamp = bootstrap.header.execution.timestamp * 1_000_000_000;
        ethereum::consensus_state::ConsensusState {
            slot: bootstrap.header.beacon.slot,
            state_root: bootstrap.header.execution.state_root,
            storage_root: self
                .provider
                .get_proof(
                    ethers::types::H160::from(self.ibc_handler_address.0),
                    vec![],
                    Some(bootstrap.header.execution.block_number.into()),
                )
                .await
                .unwrap()
                .storage_hash
                .0
                .into(),
            timestamp,
            current_sync_committee: bootstrap.current_sync_committee.aggregate_pubkey,
            next_sync_committee: light_client_update
                .next_sync_committee
                .map(|nsc| nsc.aggregate_pubkey),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum EthereumInitError {
    #[error("unable to connect to websocket")]
    Ws(#[from] WsClientError),
    #[error("provider error")]
    Provider(#[from] ProviderError),
}

impl<C: ChainSpec, S: EthereumSignersConfig> Ethereum<C, S> {
    pub async fn new(config: Config<S>) -> Result<Self, EthereumInitError> {
        let provider = Provider::new(Ws::connect(config.eth_rpc_api).await?);

        let chain_id = provider.get_chainid().await?;

        Ok(Self {
            chain_id: U256(chain_id),
            keyring: S::new(
                config.keyring,
                config.ibc_handler_address,
                chain_id.as_u64(),
                provider.clone(),
            ),
            ibc_handler_address: config.ibc_handler_address,
            provider: Arc::new(provider),
            beacon_api_client: BeaconApiClient::new(config.eth_beacon_rpc_api).await,
        })
    }
}

/// Fetch an eth_getProof call on an Ethereum-based chain that has the exact same response type as Ethereum.
pub async fn get_proof<Hc: EthereumChain>(
    hc: &Hc,
    address: H160,
    location: U256,
    block: u64,
) -> StorageProof {
    let proof = hc
        .provider()
        .get_proof(
            ethers::types::H160::from(address),
            vec![location.to_be_bytes().into()],
            Some(block.into()),
        )
        .await
        .unwrap();

    let proof = match <[_; 1]>::try_from(proof.storage_proof) {
        Ok([proof]) => proof,
        Err(invalid) => {
            panic!("received invalid response from eth_getProof, expected length of 1 but got `{invalid:#?}`");
        }
    };

    StorageProof {
        key: U256::from_be_bytes(proof.key.to_fixed_bytes()),
        value: proof.value.into(),
        proof: proof
            .proof
            .into_iter()
            .map(|bytes| bytes.to_vec())
            .collect(),
    }
}

impl<C: ChainSpec, S: EthereumSignersConfig> Ethereum<C, S> {
    pub fn make_height(&self, height: impl Into<u64>) -> Height {
        // REVIEW: Consider using the fork revision?
        {
            Height {
                revision_number: ETHEREUM_REVISION_NUMBER,
                revision_height: height.into(),
            }
        }
    }
}

pub trait IbcHandlerExt<M: Middleware> {
    fn ibc_state_read<P, Hc, Tr>(
        &self,
        execution_block_number: u64,
        path: P,
    ) -> impl Future<Output = Result<P::Value, ContractError<M>>> + Send
    where
        P: EthereumStateRead<Hc, Tr> + 'static,
        Hc: EthereumChain,
        Tr: Chain;

    fn get_client_state<Hc: EthereumChain, ClientState: Decode<EthAbi>>(
        &self,
        client_id: ClientIdOf<Hc>,
        execution_block_number: u64,
    ) -> impl Future<Output = ClientState>;
}

impl<M: Middleware> IbcHandlerExt<M> for IBCHandler<M> {
    async fn ibc_state_read<P, Hc, Tr>(
        &self,
        execution_block_number: u64,
        path: P,
    ) -> Result<P::Value, ContractError<M>>
    where
        P: EthereumStateRead<Hc, Tr> + 'static,
        Hc: EthereumChain,
        Tr: Chain,
    {
        Ok(path.read(self, execution_block_number).await)
    }

    async fn get_client_state<Hc: EthereumChain, ClientState: Decode<EthAbi>>(
        &self,
        client_id: ClientIdOf<Hc>,
        execution_block_number: u64,
    ) -> ClientState {
        let client_address = self
            .get_client(client_id.to_string())
            .block(execution_block_number)
            .call()
            .await
            .unwrap();

        let bytes = ILightClient::new(client_address, self.client())
            .get_client_state(client_id.to_string())
            .block(execution_block_number)
            .call()
            .await
            .unwrap();

        <ClientState as Decode<EthAbi>>::decode(&bytes).unwrap()
    }
}

pub type EthereumClientId = ClientId;

pub fn next_epoch_timestamp<C: ChainSpec>(slot: u64, genesis_timestamp: u64) -> u64 {
    let next_epoch_slot = slot + (C::SLOTS_PER_EPOCH::U64 - (slot % C::SLOTS_PER_EPOCH::U64));
    genesis_timestamp + (next_epoch_slot * C::SECONDS_PER_SLOT::U64)
}

pub trait EthereumStateRead<Hc, Tr>: IbcPath<Hc, Tr>
where
    Hc: EthereumChain,
    Tr: Chain,
{
    fn read<M: Middleware>(
        self,
        ibc_handler: &IBCHandler<M>,
        execution_block_number: u64,
    ) -> impl Future<Output = Self::Value> + Send + '_;
}

impl<Hc, Tr> EthereumStateRead<Hc, Tr> for ClientStatePath<ClientIdOf<Hc>>
where
    Hc: EthereumChain,
    Tr: Chain,
    Self::Value: Decode<Hc::IbcStateEncoding>,
{
    async fn read<M: Middleware>(
        self,
        ibc_handler: &IBCHandler<M>,
        execution_block_number: u64,
    ) -> Self::Value {
        ibc_handler
            .get_client_state::<Hc, Self::Value>(self.client_id, execution_block_number)
            .await
    }
}

impl<Hc, Tr> EthereumStateRead<Hc, Tr> for ClientConsensusStatePath<ClientIdOf<Hc>, HeightOf<Tr>>
where
    Hc: EthereumChain,
    Tr: Chain,
    Self::Value: Decode<EthAbi>,
{
    async fn read<M: Middleware>(
        self,
        ibc_handler: &IBCHandler<M>,
        execution_block_number: u64,
    ) -> Self::Value {
        let client_address = ibc_handler
            .get_client(self.client_id.to_string())
            .block(execution_block_number)
            .call()
            .await
            .unwrap();

        let bytes = ILightClient::new(client_address, ibc_handler.client())
            .get_consensus_state(self.client_id.to_string(), self.height.into_height().into())
            .block(execution_block_number)
            .call()
            .await
            .unwrap();

        <Self::Value as Decode<EthAbi>>::decode(&bytes).unwrap()
    }
}

impl<Hc, Tr> EthereumStateRead<Hc, Tr> for ConnectionPath
where
    Hc: EthereumChain,
    Tr: Chain,
{
    async fn read<M: Middleware>(
        self,
        ibc_handler: &IBCHandler<M>,
        execution_block_number: u64,
    ) -> Self::Value {
        ibc_handler
            .get_connection(self.connection_id.to_string())
            .block(execution_block_number)
            .call()
            .await
            .unwrap()
            .try_into()
            .unwrap()
    }
}

impl<Hc, Tr> EthereumStateRead<Hc, Tr> for ChannelEndPath
where
    Hc: EthereumChain,
    Tr: Chain,
{
    async fn read<M: Middleware>(
        self,
        ibc_handler: &IBCHandler<M>,
        execution_block_number: u64,
    ) -> Self::Value {
        let raw = ibc_handler
            .get_channel(self.port_id.to_string(), self.channel_id.to_string())
            .block(execution_block_number)
            .call()
            .await
            .unwrap();

        raw.try_into().unwrap()
    }
}

impl<Hc, Tr> EthereumStateRead<Hc, Tr> for CommitmentPath
where
    Hc: EthereumChain,
    Tr: Chain,
{
    async fn read<M: Middleware>(
        self,
        ibc_handler: &IBCHandler<M>,
        execution_block_number: u64,
    ) -> Self::Value {
        ibc_handler
            .client()
            .get_storage_at(
                ibc_handler.address(),
                commitment_key::<Hc, Tr>(self).into(),
                Some(execution_block_number.into()),
            )
            .await
            .unwrap()
            .into()
    }
}

impl<Hc, Tr> EthereumStateRead<Hc, Tr> for AcknowledgementPath
where
    Hc: EthereumChain,
    Tr: Chain,
{
    async fn read<M: Middleware>(
        self,
        ibc_handler: &IBCHandler<M>,
        execution_block_number: u64,
    ) -> Self::Value {
        ibc_handler
            .client()
            .get_storage_at(
                ibc_handler.address(),
                commitment_key::<Hc, Tr>(self).into(),
                Some(execution_block_number.into()),
            )
            .await
            .unwrap()
            .into()
    }
}

impl<Hc, Tr> EthereumStateRead<Hc, Tr> for NextConnectionSequencePath
where
    Hc: EthereumChain,
    Tr: Chain,
{
    async fn read<M: Middleware>(
        self,
        ibc_handler: &IBCHandler<M>,
        execution_block_number: u64,
    ) -> Self::Value {
        U256::from_be_bytes(
            ibc_handler
                .client()
                .get_storage_at(
                    ibc_handler.address(),
                    commitment_key::<Hc, Tr>(self).into(),
                    Some(execution_block_number.into()),
                )
                .await
                .unwrap()
                .0,
        )
        .try_into()
        .unwrap()
    }
}

impl<Hc, Tr> EthereumStateRead<Hc, Tr> for NextClientSequencePath
where
    Hc: EthereumChain,
    Tr: Chain,
{
    async fn read<M: Middleware>(
        self,
        ibc_handler: &IBCHandler<M>,
        execution_block_number: u64,
    ) -> Self::Value {
        U256::from_be_bytes(
            ibc_handler
                .client()
                .get_storage_at(
                    ibc_handler.address(),
                    commitment_key::<Hc, Tr>(self).into(),
                    Some(execution_block_number.into()),
                )
                .await
                .unwrap()
                .0,
        )
        .try_into()
        .unwrap()
    }
}

impl<Hc, Tr> EthereumStateRead<Hc, Tr> for ReceiptPath
where
    Hc: EthereumChain,
    Tr: Chain,
{
    async fn read<M: Middleware>(
        self,
        ibc_handler: &IBCHandler<M>,
        execution_block_number: u64,
    ) -> Self::Value {
        match u64::try_from(U256::from_be_bytes(
            ibc_handler
                .client()
                .get_storage_at(
                    ibc_handler.address(),
                    commitment_key::<Hc, Tr>(self).into(),
                    Some(execution_block_number.into()),
                )
                .await
                .unwrap()
                .0,
        ))
        .unwrap()
        {
            0 => false,
            1 => true,
            n => panic!("not a bool??? {n}"),
        }
    }
}

impl<Hc, Tr> EthereumStateRead<Hc, Tr> for NextSequenceRecvPath
where
    Hc: EthereumChain,
    Tr: Chain,
{
    async fn read<M: Middleware>(
        self,
        ibc_handler: &IBCHandler<M>,
        execution_block_number: u64,
    ) -> Self::Value {
        u64::try_from(U256::from_be_bytes(
            ibc_handler
                .client()
                .get_storage_at(
                    ibc_handler.address(),
                    commitment_key::<Hc, Tr>(self).into(),
                    Some(execution_block_number.into()),
                )
                .await
                .unwrap()
                .0,
        ))
        .unwrap()
    }
}

#[test]
fn eth_chain_type() {
    assert_eq!(
        <<Ethereum<unionlabs::ethereum::config::Mainnet> as Chain>::ChainType as FromStrExact>::EXPECTING,
        "eth-mainnet",
    );
    assert_eq!(
        <<Ethereum<unionlabs::ethereum::config::Minimal> as Chain>::ChainType as FromStrExact>::EXPECTING,
        "eth-minimal",
    );
}

// TODO: Remove this in favor of the one in unionlabs
pub fn commitment_key<Hc: Chain, Tr: Chain>(path: impl IbcPath<Hc, Tr>) -> H256 {
    keccak256(
        keccak256(path.to_string())
            .into_iter()
            .chain(AbiEncode::encode(IBC_HANDLER_COMMITMENTS_SLOT))
            .collect::<Vec<_>>(),
    )
    .into()
}
