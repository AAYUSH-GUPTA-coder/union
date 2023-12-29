use serde::{Deserialize, Serialize};

#[cfg(feature = "ethabi")]
use crate::InlineFields;
use crate::{
    errors::{required, InvalidLength, MissingField},
    hash::H256,
    ibc::core::commitment::merkle_root::MerkleRoot,
    Proto, TryFromProtoErrorOf, TypeUrl,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConsensusState {
    pub timestamp: u64,
    pub root: MerkleRoot,
    pub next_validators_hash: H256,
}

#[derive(Debug)]
pub enum TryFromConsensusStateError {
    MissingField(MissingField),
    Root(TryFromProtoErrorOf<MerkleRoot>),
    NextValidatorsHash(InvalidLength),
}

impl TryFrom<protos::union::ibc::lightclients::cometbls::v1::ConsensusState> for ConsensusState {
    type Error = TryFromConsensusStateError;

    fn try_from(
        value: protos::union::ibc::lightclients::cometbls::v1::ConsensusState,
    ) -> Result<Self, Self::Error> {
        Ok(Self {
            timestamp: value.timestamp,
            root: required!(value.root)?
                .try_into()
                .map_err(TryFromConsensusStateError::Root)?,
            next_validators_hash: value
                .next_validators_hash
                .try_into()
                .map_err(TryFromConsensusStateError::NextValidatorsHash)?,
        })
    }
}

#[cfg(feature = "ethabi")]
impl crate::EthAbi for ConsensusState {
    type EthAbi = contracts::glue::OptimizedConsensusState;
}

impl TypeUrl for protos::union::ibc::lightclients::cometbls::v1::ConsensusState {
    const TYPE_URL: &'static str = "/union.ibc.lightclients.cometbls.v1.ConsensusState";
}

impl Proto for ConsensusState {
    type Proto = protos::union::ibc::lightclients::cometbls::v1::ConsensusState;
}

impl From<ConsensusState> for protos::union::ibc::lightclients::cometbls::v1::ConsensusState {
    fn from(value: ConsensusState) -> Self {
        Self {
            timestamp: value.timestamp,
            root: Some(value.root.into()),
            next_validators_hash: value.next_validators_hash.into(),
        }
    }
}

#[cfg(feature = "ethabi")]
impl From<ConsensusState> for contracts::glue::OptimizedConsensusState {
    fn from(value: ConsensusState) -> Self {
        Self {
            timestamp: value.timestamp,
            root: value.root.hash.into(),
            next_validators_hash: value.next_validators_hash.into(),
        }
    }
}

#[cfg(feature = "ethabi")]
impl From<ConsensusState> for InlineFields<contracts::glue::OptimizedConsensusState> {
    fn from(value: ConsensusState) -> Self {
        Self(value.into())
    }
}

#[cfg(feature = "ethabi")]
impl TryFrom<contracts::glue::OptimizedConsensusState> for ConsensusState {
    type Error = TryFromConsensusStateError;

    fn try_from(value: contracts::glue::OptimizedConsensusState) -> Result<Self, Self::Error> {
        Ok(Self {
            timestamp: value.timestamp,
            root: MerkleRoot::from(H256::from(value.root)),
            next_validators_hash: value.next_validators_hash.into(),
        })
    }
}

#[cfg(feature = "ethabi")]
impl TryFrom<InlineFields<contracts::glue::OptimizedConsensusState>> for ConsensusState {
    type Error = TryFromConsensusStateError;

    fn try_from(
        value: InlineFields<contracts::glue::OptimizedConsensusState>,
    ) -> Result<Self, Self::Error> {
        value.0.try_into()
    }
}
