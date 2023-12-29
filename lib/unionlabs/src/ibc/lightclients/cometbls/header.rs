use std::fmt::Debug;

use serde::{Deserialize, Serialize};

#[cfg(feature = "ethabi")]
use crate::InlineFields;
use crate::{
    errors::{InvalidLength, MissingField},
    ibc::core::client::height::Height,
    tendermint::types::signed_header::SignedHeader,
    Proto, TryFromProtoErrorOf, TypeUrl,
};

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub signed_header: SignedHeader,
    pub trusted_height: Height,
    #[serde(with = "::serde_utils::hex_string")]
    pub zero_knowledge_proof: Vec<u8>,
}

impl Debug for Header {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Header")
            .field("signed_header", &self.signed_header)
            .field("trusted_height", &self.trusted_height)
            .field(
                "zero_knowledge_proof",
                &serde_utils::to_hex(&self.zero_knowledge_proof),
            )
            .finish()
    }
}

impl From<Header> for protos::union::ibc::lightclients::cometbls::v1::Header {
    fn from(value: Header) -> Self {
        Self {
            signed_header: Some(value.signed_header.into()),
            trusted_height: Some(value.trusted_height.into()),
            zero_knowledge_proof: value.zero_knowledge_proof,
        }
    }
}

#[cfg(feature = "ethabi")]
impl From<Header> for contracts::glue::UnionIbcLightclientsCometblsV1HeaderData {
    fn from(value: Header) -> Self {
        Self {
            signed_header: value.signed_header.into(),
            trusted_height: value.trusted_height.into(),
            zero_knowledge_proof: value.zero_knowledge_proof.into(),
        }
    }
}

#[cfg(feature = "ethabi")]
impl From<Header> for InlineFields<contracts::glue::UnionIbcLightclientsCometblsV1HeaderData> {
    fn from(value: Header) -> Self {
        Self(value.into())
    }
}

#[cfg(feature = "ethabi")]
impl crate::EthAbi for Header {
    type EthAbi = InlineFields<contracts::glue::UnionIbcLightclientsCometblsV1HeaderData>;
}

#[derive(Debug)]
pub enum TryFromHeaderError {
    MissingField(MissingField),
    SignedHeader(TryFromProtoErrorOf<SignedHeader>),
    UntrustedValidatorSetRoot(InvalidLength),
}

impl TryFrom<protos::union::ibc::lightclients::cometbls::v1::Header> for Header {
    type Error = TryFromHeaderError;

    fn try_from(
        value: protos::union::ibc::lightclients::cometbls::v1::Header,
    ) -> Result<Self, Self::Error> {
        Ok(Self {
            signed_header: value
                .signed_header
                .ok_or(TryFromHeaderError::MissingField(MissingField(
                    "signed header",
                )))?
                .try_into()
                .map_err(TryFromHeaderError::SignedHeader)?,
            trusted_height: value
                .trusted_height
                .ok_or(TryFromHeaderError::MissingField(MissingField(
                    "trusted height",
                )))?
                .into(),
            zero_knowledge_proof: value.zero_knowledge_proof,
        })
    }
}

impl Proto for Header {
    type Proto = protos::union::ibc::lightclients::cometbls::v1::Header;
}

impl TypeUrl for protos::union::ibc::lightclients::cometbls::v1::Header {
    const TYPE_URL: &'static str = "/union.ibc.lightclients.cometbls.v1.Header";
}
