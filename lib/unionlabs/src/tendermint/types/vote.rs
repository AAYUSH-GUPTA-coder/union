use crate::{
    bounded::{BoundedI32, BoundedI64},
    google::protobuf::timestamp::Timestamp,
    hash::H160,
    tendermint::types::{block_id::BlockId, signed_msg_type::SignedMsgType},
    Proto,
};

pub struct Vote {
    pub r#type: SignedMsgType,
    pub height: BoundedI64<0, { i64::MAX }>,
    pub round: BoundedI32<0, { i32::MAX }>,
    pub block_id: BlockId,
    pub timestamp: Timestamp,
    // TODO(aeryz): is this H160 or bytes
    pub validator_address: H160,
    pub validator_index: i32,
    pub signature: Vec<u8>,
    pub extension: Vec<u8>,
    pub extension_signature: Vec<u8>,
}

impl Proto for Vote {
    type Proto = protos::tendermint::types::Vote;
}

impl From<Vote> for protos::tendermint::types::Vote {
    fn from(value: Vote) -> Self {
        Self {
            r#type: value.r#type.into(),
            height: value.height.inner(),
            round: value.round.inner(),
            block_id: Some(value.block_id.into()),
            timestamp: Some(value.timestamp.into()),
            validator_address: value.validator_address.into(),
            validator_index: value.validator_index,
            signature: value.signature,
            extension: value.extension,
            extension_signature: value.extension_signature,
        }
    }
}
