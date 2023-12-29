use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommissionRates {
    /// rate is the commission rate charged to delegators, as a fraction.
    pub rate: String,
    /// max_rate defines the maximum commission rate which validator can ever charge, as a fraction.
    pub max_rate: String,
    /// max_change_rate defines the maximum daily increase of the validator commission, as a fraction.
    pub max_change_rate: String,
}

impl From<protos::cosmos::staking::v1beta1::CommissionRates> for CommissionRates {
    fn from(value: protos::cosmos::staking::v1beta1::CommissionRates) -> Self {
        Self {
            rate: value.rate,
            max_rate: value.max_rate,
            max_change_rate: value.max_change_rate,
        }
    }
}

impl From<CommissionRates> for protos::cosmos::staking::v1beta1::CommissionRates {
    fn from(value: CommissionRates) -> Self {
        Self {
            rate: value.rate,
            max_rate: value.max_rate,
            max_change_rate: value.max_change_rate,
        }
    }
}
