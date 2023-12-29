use serde::{Deserialize, Serialize};

use crate::{
    cosmos::staking::commission_rates::CommissionRates,
    errors::{required, MissingField},
    google::protobuf::timestamp::Timestamp,
    Proto, TryFromProtoErrorOf, TypeUrl,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Commission {
    /// commission_rates defines the initial commission rates to be used for creating a validator.
    pub commission_rates: CommissionRates,
    /// update_time is the last time the commission rate was changed.
    pub update_time: Timestamp,
}

impl Proto for Commission {
    type Proto = protos::cosmos::staking::v1beta1::Commission;
}

impl TypeUrl for protos::cosmos::staking::v1beta1::Commission {
    const TYPE_URL: &'static str = "/cosmos.staking.v1beta1.Commission";
}

#[derive(Debug)]
pub enum TryFromCommissionError {
    MissingField(MissingField),
    Timestamp(TryFromProtoErrorOf<Timestamp>),
}

impl TryFrom<protos::cosmos::staking::v1beta1::Commission> for Commission {
    type Error = TryFromCommissionError;

    fn try_from(value: protos::cosmos::staking::v1beta1::Commission) -> Result<Self, Self::Error> {
        Ok(Self {
            commission_rates: required!(value.commission_rates)?.into(),
            update_time: required!(value.update_time)?
                .try_into()
                .map_err(TryFromCommissionError::Timestamp)?,
        })
    }
}

impl From<Commission> for protos::cosmos::staking::v1beta1::Commission {
    fn from(value: Commission) -> Self {
        Self {
            commission_rates: Some(value.commission_rates.into()),
            update_time: Some(value.update_time.into()),
        }
    }
}
