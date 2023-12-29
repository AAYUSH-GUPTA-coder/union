use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::id::{ChannelId, PortId};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counterparty {
    pub port_id: PortId,
    pub channel_id: ChannelId,
}

impl From<Counterparty> for protos::ibc::core::channel::v1::Counterparty {
    fn from(value: Counterparty) -> Self {
        Self {
            port_id: value.port_id.to_string(),
            channel_id: value.channel_id.to_string(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TryFromChannelCounterpartyError {
    #[error("error parsing port id")]
    PortId(#[source] <PortId as FromStr>::Err),
    #[error("error parsing channel id")]
    ChannelId(#[source] <ChannelId as FromStr>::Err),
}

impl TryFrom<protos::ibc::core::channel::v1::Counterparty> for Counterparty {
    type Error = TryFromChannelCounterpartyError;

    fn try_from(proto: protos::ibc::core::channel::v1::Counterparty) -> Result<Self, Self::Error> {
        Ok(Self {
            port_id: proto
                .port_id
                .parse()
                .map_err(TryFromChannelCounterpartyError::PortId)?,
            channel_id: proto
                .channel_id
                .parse()
                .map_err(TryFromChannelCounterpartyError::ChannelId)?,
        })
    }
}

#[cfg(feature = "ethabi")]
impl From<Counterparty> for contracts::ibc_handler::IbcCoreChannelV1CounterpartyData {
    fn from(value: Counterparty) -> Self {
        Self {
            port_id: value.port_id.to_string(),
            channel_id: value.channel_id.to_string(),
        }
    }
}

#[cfg(feature = "ethabi")]
#[derive(Debug, thiserror::Error)]
pub enum TryFromEthAbiChannelCounterpartyError {
    #[error("error parsing port id")]
    PortId(#[source] <PortId as FromStr>::Err),
    #[error("error parsing channel id")]
    ChannelId(#[source] <ChannelId as FromStr>::Err),
}

#[cfg(feature = "ethabi")]
impl TryFrom<contracts::ibc_handler::IbcCoreChannelV1CounterpartyData> for Counterparty {
    type Error = TryFromChannelCounterpartyError;

    fn try_from(
        value: contracts::ibc_handler::IbcCoreChannelV1CounterpartyData,
    ) -> Result<Self, Self::Error> {
        Ok(Self {
            port_id: value
                .port_id
                .parse()
                .map_err(TryFromChannelCounterpartyError::PortId)?,
            channel_id: value
                .channel_id
                .parse()
                .map_err(TryFromChannelCounterpartyError::ChannelId)?,
        })
    }
}

#[cfg(feature = "ethabi")]
impl crate::EthAbi for Counterparty {
    type EthAbi = contracts::ibc_handler::IbcCoreChannelV1CounterpartyData;
}
