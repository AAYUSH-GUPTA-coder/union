use std::fmt::Debug;

use hex_literal::hex;
use serde::{Deserialize, Serialize};
use ssz::{Decode, Encode};
use ssz_types::{BitList, FixedVector, VariableList};
use tree_hash::TreeHash;

use crate::{
    bls::{BlsPublicKey, BlsSignature},
    ethereum::{
        beacon::BeaconBlock,
        config::{
            BYTES_PER_LOGS_BLOOM, DEPOSIT_CONTRACT_TREE_DEPTH, MAX_ATTESTATIONS,
            MAX_ATTESTER_SLASHINGS, MAX_BLS_TO_EXECUTION_CHANGES, MAX_BYTES_PER_TRANSACTION,
            MAX_DEPOSITS, MAX_EXTRA_DATA_BYTES, MAX_PROPOSER_SLASHINGS,
            MAX_TRANSACTIONS_PER_PAYLOAD, MAX_VALIDATORS_PER_COMMITTEE, MAX_VOLUNTARY_EXITS,
            MAX_WITHDRAWALS_PER_PAYLOAD, SYNC_COMMITTEE_SIZE,
        },
    },
    hash::H256,
    ibc::lightclients::ethereum::beacon_block_header::BeaconBlockHeader,
    macros::hex_string_array_wrapper,
};

pub mod beacon;
pub mod config;

// REVIEW: Is this needed? Currently unused
pub const BLOCK_BODY_EXECUTION_PAYLOAD_INDEX: usize = 9;

hex_string_array_wrapper! {
    pub struct Version(pub [u8; 4]);
    pub struct DomainType(pub [u8; 4]);
    pub struct ForkDigest(pub [u8; 4]);
    pub struct Domain(pub [u8; 32]);
}

#[rustfmt::skip]
impl DomainType {
    pub const BEACON_PROPOSER: Self                = Self(hex!("00000000"));
    pub const BEACON_ATTESTER: Self                = Self(hex!("01000000"));
    pub const RANDAO: Self                         = Self(hex!("02000000"));
    pub const DEPOSIT: Self                        = Self(hex!("03000000"));
    pub const VOLUNTARY_EXIT: Self                 = Self(hex!("04000000"));
    pub const SELECTION_PROOF: Self                = Self(hex!("05000000"));
    pub const AGGREGATE_AND_PROOF: Self            = Self(hex!("06000000"));
    pub const SYNC_COMMITTEE: Self                 = Self(hex!("07000000"));
    pub const SYNC_COMMITTEE_SELECTION_PROOF: Self = Self(hex!("08000000"));
    pub const CONTRIBUTION_AND_PROOF: Self         = Self(hex!("09000000"));
    pub const BLS_TO_EXECUTION_CHANGE: Self        = Self(hex!("0A000000"));
    pub const APPLICATION_MASK: Self               = Self(hex!("00000001"));
}

#[derive(Clone, Debug, PartialEq, Encode, Decode, TreeHash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForkData {
    pub current_version: Version,
    pub genesis_validators_root: H256,
}

/// <https://github.com/ethereum/consensus-specs/blob/dev/specs/phase0/beacon-chain.md#signingdata>
#[derive(Clone, Debug, PartialEq, Encode, Decode, TreeHash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SigningData {
    pub object_root: H256,
    pub domain: Domain,
}

/// <https://github.com/ethereum/consensus-specs/blob/dev/specs/phase0/beacon-chain.md#signedbeaconblockheader>
#[derive(Clone, Debug, PartialEq, Encode, Decode, TreeHash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedBeaconBlockHeader {
    pub message: BeaconBlockHeader,
    pub signature: BlsSignature,
}

/// <https://github.com/ethereum/consensus-specs/blob/dev/specs/phase0/beacon-chain.md#signedbeaconblock>
#[derive(Clone, Debug, PartialEq, Encode, Decode, TreeHash, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""), deny_unknown_fields)]
pub struct SignedBeaconBlock<
    C: MAX_PROPOSER_SLASHINGS
        + MAX_VALIDATORS_PER_COMMITTEE
        + MAX_ATTESTER_SLASHINGS
        + MAX_ATTESTATIONS
        + DEPOSIT_CONTRACT_TREE_DEPTH
        + MAX_DEPOSITS
        + MAX_VOLUNTARY_EXITS
        + BYTES_PER_LOGS_BLOOM
        + MAX_EXTRA_DATA_BYTES
        + MAX_BYTES_PER_TRANSACTION
        + MAX_TRANSACTIONS_PER_PAYLOAD
        + MAX_WITHDRAWALS_PER_PAYLOAD
        + MAX_BLS_TO_EXECUTION_CHANGES
        + SYNC_COMMITTEE_SIZE,
> {
    pub message: BeaconBlock<C>,
    pub signature: BlsSignature,
}

/// <https://github.com/ethereum/consensus-specs/blob/dev/specs/phase0/beacon-chain.md#eth1data>
#[derive(Clone, Debug, PartialEq, Encode, Decode, TreeHash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Eth1Data {
    pub deposit_root: H256,
    #[serde(with = "::serde_utils::string")]
    pub deposit_count: u64,
    pub block_hash: H256,
}

/// <https://github.com/ethereum/consensus-specs/blob/dev/specs/phase0/beacon-chain.md#proposerslashing>
#[derive(Clone, Debug, PartialEq, Encode, Decode, TreeHash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposerSlashing {
    pub signed_header_1: SignedBeaconBlockHeader,
    pub signed_header_2: SignedBeaconBlockHeader,
}

/// <https://github.com/ethereum/consensus-specs/blob/dev/specs/phase0/beacon-chain.md#attesterslashing>
#[derive(Clone, Debug, PartialEq, Encode, Decode, TreeHash, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""), deny_unknown_fields)]
pub struct AttesterSlashing<C: MAX_VALIDATORS_PER_COMMITTEE> {
    pub attestation_1: IndexedAttestation<C>,
    pub attestation_2: IndexedAttestation<C>,
}

/// <https://github.com/ethereum/consensus-specs/blob/dev/specs/phase0/beacon-chain.md#indexedattestation>
#[derive(Clone, Debug, PartialEq, Encode, Decode, TreeHash, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""), deny_unknown_fields)]
pub struct IndexedAttestation<C: MAX_VALIDATORS_PER_COMMITTEE> {
    pub attesting_indices: VariableList<u64, C::MAX_VALIDATORS_PER_COMMITTEE>,
    pub data: AttestationData,
    pub signature: BlsSignature,
}

/// <https://github.com/ethereum/consensus-specs/blob/dev/specs/phase0/beacon-chain.md#attestationdata>
#[derive(Clone, Debug, PartialEq, Encode, Decode, TreeHash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttestationData {
    #[serde(with = "::serde_utils::string")]
    pub slot: u64,
    #[serde(with = "::serde_utils::string")]
    pub index: u64,
    /// LMD GHOST vote
    pub beacon_block_root: H256,
    /// FFG vote
    pub source: Checkpoint,
    pub target: Checkpoint,
}

/// <https://github.com/ethereum/consensus-specs/blob/dev/specs/phase0/beacon-chain.md#checkpoint>
#[derive(Clone, Debug, PartialEq, Encode, Decode, TreeHash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    #[serde(with = "::serde_utils::string")]
    pub epoch: u64,
    pub root: H256,
}

/// <https://github.com/ethereum/consensus-specs/blob/dev/specs/phase0/beacon-chain.md#attestation>
#[derive(Clone, Debug, PartialEq, Encode, Decode, TreeHash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attestation<C: MAX_VALIDATORS_PER_COMMITTEE> {
    pub aggregation_bits: BitList<C::MAX_VALIDATORS_PER_COMMITTEE>,
    pub data: AttestationData,
    pub signature: BlsSignature,
}

/// <https://github.com/ethereum/consensus-specs/blob/dev/specs/phase0/beacon-chain.md#deposit>
#[derive(Clone, Debug, PartialEq, Encode, Decode, TreeHash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deposit<C: DEPOSIT_CONTRACT_TREE_DEPTH> {
    /// Merkle path to deposit root
    pub proof: FixedVector<[u8; 32], C::DEPOSIT_CONTRACT_TREE_DEPTH>,
    pub data: DepositData,
}

/// <https://github.com/ethereum/consensus-specs/blob/dev/specs/phase0/beacon-chain.md#depositdata>
#[derive(Clone, Debug, PartialEq, Encode, Decode, TreeHash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DepositData {
    pub pubkey: BlsPublicKey,
    pub withdrawal_credentials: [u8; 32],
    #[serde(with = "::serde_utils::string")]
    pub amount: u64,
    /// Signing over DepositMessage
    pub signature: BlsSignature,
}

/// <https://github.com/ethereum/consensus-specs/blob/dev/specs/phase0/beacon-chain.md#signedvoluntaryexit>
#[derive(Clone, Debug, PartialEq, Encode, Decode, TreeHash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedVoluntaryExit {
    pub message: VoluntaryExit,
    pub signature: BlsSignature,
}

/// <https://github.com/ethereum/consensus-specs/blob/dev/specs/phase0/beacon-chain.md#voluntaryexit>
#[derive(Clone, Debug, PartialEq, Encode, Decode, TreeHash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoluntaryExit {
    /// Earliest epoch when voluntary exit can be processed
    #[serde(with = "::serde_utils::string")]
    pub epoch: u64,
    #[serde(with = "::serde_utils::string")]
    pub validator_index: u64,
}
