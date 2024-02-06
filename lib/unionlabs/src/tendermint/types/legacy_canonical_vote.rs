use serde::{Deserialize, Serialize};

use crate::{
    bounded::BoundedI64,
    google::protobuf::timestamp::Timestamp,
    tendermint::types::{canonical_block_id::CanonicalBlockId, signed_msg_type::SignedMsgType},
    Proto, TypeUrl,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyCanonicalVote {
    /// type alias for byte
    pub ty: SignedMsgType,
    /// canonicalization requires fixed size encoding here
    pub height: BoundedI64<0, { i64::MAX }>,
    /// canonicalization requires fixed size encoding here
    pub round: BoundedI64<0, { i64::MAX }>,
    pub block_id: CanonicalBlockId,
    pub chain_id: String,
    pub timestamp: Timestamp,
}

impl Proto for LegacyCanonicalVote {
    type Proto = protos::tendermint::types::LegacyCanonicalVote;
}

impl TypeUrl for protos::tendermint::types::LegacyCanonicalVote {
    const TYPE_URL: &'static str = "/tendermint.types.LegacyCanonicalVote";
}

impl From<LegacyCanonicalVote> for protos::tendermint::types::LegacyCanonicalVote {
    fn from(value: LegacyCanonicalVote) -> Self {
        Self {
            r#type: value.ty.into(),
            height: value.height.into(),
            round: value.round.into(),
            block_id: Some(value.block_id.into()),
            chain_id: value.chain_id,
            timestamp: Some(value.timestamp.into()),
        }
    }
}
