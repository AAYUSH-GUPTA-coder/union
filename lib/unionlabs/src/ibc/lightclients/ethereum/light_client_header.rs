use serde::{Deserialize, Serialize};
use ssz::{Decode, Encode};
use ssz_types::{fixed_vector, FixedVector};
use tree_hash::TreeHash;
use typenum::U;

use crate::{
    errors::{InvalidLength, MissingField},
    ethereum::config::{
        consts::{floorlog2, EXECUTION_PAYLOAD_INDEX},
        BYTES_PER_LOGS_BLOOM, MAX_EXTRA_DATA_BYTES,
    },
    hash::H256,
    ibc::lightclients::ethereum::{
        beacon_block_header::BeaconBlockHeader, execution_payload_header::ExecutionPayloadHeader,
    },
    Proto, TryFromProtoErrorOf, TypeUrl,
};

#[derive(Clone, Debug, PartialEq, Encode, Decode, TreeHash, Serialize, Deserialize)]
#[serde(bound(serialize = "", deserialize = ""), deny_unknown_fields)]
pub struct LightClientHeader<C: BYTES_PER_LOGS_BLOOM + MAX_EXTRA_DATA_BYTES> {
    pub beacon: BeaconBlockHeader,
    pub execution: ExecutionPayloadHeader<C>,
    pub execution_branch: FixedVector<H256, U<{ floorlog2(EXECUTION_PAYLOAD_INDEX) }>>,
}

impl<C: BYTES_PER_LOGS_BLOOM + MAX_EXTRA_DATA_BYTES> From<LightClientHeader<C>>
    for protos::union::ibc::lightclients::ethereum::v1::LightClientHeader
{
    fn from(value: LightClientHeader<C>) -> Self {
        Self {
            beacon: Some(value.beacon.into()),
            execution: Some(value.execution.into()),
            execution_branch: Vec::from(value.execution_branch)
                .into_iter()
                .map(H256::into_bytes)
                .collect(),
        }
    }
}

#[derive(Debug)]
pub enum TryFromLightClientHeaderError<C: BYTES_PER_LOGS_BLOOM + MAX_EXTRA_DATA_BYTES> {
    MissingField(MissingField),
    BeaconBlockHeader(TryFromProtoErrorOf<BeaconBlockHeader>),
    ExecutionPayloadHeader(TryFromProtoErrorOf<ExecutionPayloadHeader<C>>),
    ExecutionBranch(fixed_vector::TryFromVecError),
    ExecutionBranchNode(InvalidLength),
}

impl<C: BYTES_PER_LOGS_BLOOM + MAX_EXTRA_DATA_BYTES>
    TryFrom<protos::union::ibc::lightclients::ethereum::v1::LightClientHeader>
    for LightClientHeader<C>
{
    type Error = TryFromLightClientHeaderError<C>;

    fn try_from(
        value: protos::union::ibc::lightclients::ethereum::v1::LightClientHeader,
    ) -> Result<Self, Self::Error> {
        Ok(Self {
            beacon: value
                .beacon
                .ok_or(TryFromLightClientHeaderError::MissingField(MissingField(
                    "beacon",
                )))?
                .try_into()
                .map_err(TryFromLightClientHeaderError::BeaconBlockHeader)?,
            execution: value
                .execution
                .ok_or(TryFromLightClientHeaderError::MissingField(MissingField(
                    "execution",
                )))?
                .try_into()
                .map_err(TryFromLightClientHeaderError::ExecutionPayloadHeader)?,
            execution_branch: value
                .execution_branch
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<Vec<_>, _>>()
                .map_err(TryFromLightClientHeaderError::ExecutionBranchNode)?
                .try_into()
                .map_err(TryFromLightClientHeaderError::ExecutionBranch)?,
        })
    }
}

impl TypeUrl for protos::union::ibc::lightclients::ethereum::v1::LightClientHeader {
    const TYPE_URL: &'static str = "/union.ibc.lightclients.ethereum.v1.LightClientHeader";
}

impl<C: BYTES_PER_LOGS_BLOOM + MAX_EXTRA_DATA_BYTES> Proto for LightClientHeader<C> {
    type Proto = protos::union::ibc::lightclients::ethereum::v1::LightClientHeader;
}
