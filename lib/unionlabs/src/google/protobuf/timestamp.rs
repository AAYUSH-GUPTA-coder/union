use core::num::TryFromIntError;
use std::{
    cmp::Ordering,
    fmt::Display,
    ops::{Div, Neg},
    str::FromStr,
};

use chrono::{DateTime, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use serde::{
    de::{self, Unexpected},
    Deserialize, Serialize,
};

use crate::{
    bounded::{BoundedI32, BoundedI64, BoundedIntError},
    google::protobuf::duration::{Duration, NANOS_PER_SECOND},
    Proto, TypeUrl,
};

/// See <https://play.rust-lang.org/?version=stable&mode=debug&edition=2021&gist=c27d92ace805175896bb68664bb492b6>
pub const TIMESTAMP_SECONDS_MAX: i64 = 253_402_300_799;
pub const TIMESTAMP_SECONDS_MIN: i64 = -62_135_596_800;

const NANOS_MAX: i32 = 999_999_999;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
pub struct Timestamp {
    /// As per the proto docs: "Must be from 0001-01-01T00:00:00Z to
    /// 9999-12-31T23:59:59Z inclusive."
    pub seconds: BoundedI64<TIMESTAMP_SECONDS_MIN, TIMESTAMP_SECONDS_MAX>,
    // As per the proto docs: "Must be from 0 to 999,999,999 inclusive."
    pub nanos: BoundedI32<0, NANOS_MAX>,
}

impl Ord for Timestamp {
    fn cmp(&self, other: &Self) -> Ordering {
        self.seconds
            .cmp(&other.seconds)
            .then_with(|| self.nanos.cmp(&other.nanos))
    }
}

impl PartialOrd for Timestamp {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        String::deserialize(deserializer).and_then(|str| {
            str.parse().map_err(|_| {
                de::Error::invalid_value(Unexpected::Str(&str), &"a valid RFC 3339 string")
            })
        })
    }
}

impl Serialize for Timestamp {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(self)
    }
}

impl Timestamp {
    #[must_use]
    /// Returns the duration between `self` and `other`. If `self` > `other`, the
    /// resulting [`Duration`] will be positive, and if `other` > `self` then the
    /// resulting [`Duration`] will be negative.
    pub fn duration_since(&self, other: &Self) -> Option<Duration> {
        match self.cmp(other) {
            Ordering::Greater => {
                let mut seconds = self.seconds.inner().checked_sub(other.seconds.inner())?;

                let nanos = if self.nanos < other.nanos {
                    seconds -= 1;

                    NANOS_PER_SECOND - (other.nanos.inner() - self.nanos.inner())
                } else {
                    self.nanos.inner() - other.nanos.inner()
                };

                Duration::new(seconds, nanos).ok()
            }
            Ordering::Equal => Duration::new(0, 0).ok(),
            Ordering::Less => other.duration_since(self).map(Neg::neg),
        }
    }

    pub fn checked_add(&self, duration: Duration) -> Option<Timestamp> {
        let mut seconds = self
            .seconds
            .inner()
            .checked_add(duration.seconds().inner())?;

        // No need to do checked_add here since MAX and MIN values of this
        // addition is within the bounds of i32
        let mut nanos = self.nanos.inner() + duration.nanos().inner();

        if nanos < 0 {
            nanos += NANOS_MAX + 1;
            seconds -= 1;
        } else if nanos > NANOS_MAX {
            // Subtract instead of mod since we know that NANOS cannot be greater
            // than 2 * NANOS_MAX;
            nanos -= NANOS_MAX + 1;
            seconds += 1;
        }

        match (seconds.try_into(), nanos.try_into()) {
            (Ok(seconds), Ok(nanos)) => Some(Timestamp { seconds, nanos }),
            _ => None,
        }
    }
}

impl Display for Timestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&DateTime::<Utc>::from(*self).to_rfc3339_opts(
            SecondsFormat::Nanos,
            // use_z
            true,
        ))
    }
}

impl From<Timestamp> for DateTime<Utc> {
    fn from(value: Timestamp) -> Self {
        DateTime::<Utc>::from_naive_utc_and_offset(
            NaiveDateTime::from_timestamp_opt(
                value.seconds.inner(),
                value
                    .nanos
                    .inner()
                    .try_into()
                    .expect("nanos bounds are within the bounds of u32; qed;"),
            )
            .expect("values are within bounds; qed;"),
            Utc,
        )
    }
}

#[derive(Debug)]
pub enum TryFromDateTimeError {
    Seconds(BoundedIntError<i64>),
}

impl<Tz: TimeZone> TryFrom<DateTime<Tz>> for Timestamp {
    type Error = TryFromDateTimeError;

    fn try_from(value: DateTime<Tz>) -> Result<Self, Self::Error> {
        let mut seconds = value.timestamp();
        let mut nanos: i32 = value.timestamp_subsec_nanos().try_into().expect(
            "timestamp_subsec_nanos returns a value in 0..=1_999_999_999, which is in range of i32; qed;",
        );

        if nanos >= NANOS_PER_SECOND {
            nanos -= NANOS_PER_SECOND;

            debug_assert!(NaiveDateTime::MAX.timestamp() < i64::MAX);

            // REVIEW: is this expected behaviour for leap seconds? The proto docs
            // mention [smear](https://developers.google.com/time/smear) but I'm
            // not sure what to do with potential leap seconds in this context,
            // especially since chrono doesn't make any guarantees about when or
            // where they will fall (i.e. any value in 0..=1_999_999_999 is a valid
            // nanos value).
            seconds += 1;
        }

        Ok(Self {
            seconds: seconds.try_into().map_err(TryFromDateTimeError::Seconds)?,
            nanos: nanos
                .try_into()
                .expect("nanos is within 0..=999_999_999; qed;"),
        })
    }
}

#[derive(Debug)]
pub enum TimestampFromStrError {
    Parse(chrono::ParseError),
    OutOfRange(TryFromDateTimeError),
}

impl FromStr for Timestamp {
    type Err = TimestampFromStrError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        DateTime::parse_from_rfc3339(s)
            .map_err(TimestampFromStrError::Parse)
            .and_then(|dt| dt.try_into().map_err(TimestampFromStrError::OutOfRange))
    }
}

impl Proto for Timestamp {
    type Proto = protos::google::protobuf::Timestamp;
}

impl TypeUrl for protos::google::protobuf::Timestamp {
    const TYPE_URL: &'static str = "/google.protobuf.Timestamp";
}

#[derive(Debug)]
pub enum TryFromCosmwasmTimestampError {
    Seconds(BoundedIntError<i64>),
    Nanos(BoundedIntError<i32>),
    IntCast(TryFromIntError),
}

impl TryFrom<cosmwasm_std::Timestamp> for Timestamp {
    type Error = TryFromCosmwasmTimestampError;

    fn try_from(value: cosmwasm_std::Timestamp) -> Result<Self, Self::Error> {
        Ok(Self {
            seconds: TryInto::<i64>::try_into(value.seconds())
                .map_err(TryFromCosmwasmTimestampError::IntCast)?
                .try_into()
                .map_err(TryFromCosmwasmTimestampError::Seconds)?,
            nanos: TryInto::<i32>::try_into(value.nanos())
                .map_err(TryFromCosmwasmTimestampError::IntCast)?
                .try_into()
                .map_err(TryFromCosmwasmTimestampError::Nanos)?,
        })
    }
}

impl From<Timestamp> for cosmwasm_std::Timestamp {
    fn from(value: Timestamp) -> Self {
        // REVIEW(aeryz): I always expect timestamp to be non-negative integer, that's
        // why `unwrap`ping seems like the right way to go, please give me a heads up
        // if there is an exception and we should convert this implementation to
        // `TryFrom` instead.
        cosmwasm_std::Timestamp::from_seconds(
            value
                .seconds
                .inner()
                .try_into()
                .expect("impossible since this is always inbounds"),
        )
        .plus_nanos(
            value
                .nanos
                .inner()
                .try_into()
                .expect("impossible since this is always inbounds"),
        )
    }
}

impl From<Timestamp> for protos::google::protobuf::Timestamp {
    fn from(value: Timestamp) -> Self {
        Self {
            seconds: value.seconds.into(),
            nanos: value.nanos.into(),
        }
    }
}

#[derive(Debug)]
pub enum TryFromTimestampError {
    Seconds(BoundedIntError<i64>),
    Nanos(BoundedIntError<i32>),
}

impl TryFrom<protos::google::protobuf::Timestamp> for Timestamp {
    type Error = TryFromTimestampError;

    fn try_from(value: protos::google::protobuf::Timestamp) -> Result<Self, Self::Error> {
        Ok(Self {
            seconds: value
                .seconds
                .try_into()
                .map_err(TryFromTimestampError::Seconds)?,
            nanos: value
                .nanos
                .try_into()
                .map_err(TryFromTimestampError::Nanos)?,
        })
    }
}

#[cfg(feature = "ethabi")]
impl From<Timestamp> for contracts::glue::GoogleProtobufTimestampData {
    fn from(value: Timestamp) -> Self {
        Self {
            secs: value.seconds.into(),
            nanos: value.nanos.inner().into(),
        }
    }
}

#[cfg(feature = "ethabi")]
#[derive(Debug)]
pub enum TryFromEthAbiTimestampError {
    Seconds(BoundedIntError<i64>),
    Nanos(BoundedIntError<i32>),
    NanosTryFromI64(std::num::TryFromIntError),
}

#[cfg(feature = "ethabi")]
impl TryFrom<contracts::glue::GoogleProtobufTimestampData> for Timestamp {
    type Error = TryFromEthAbiTimestampError;

    fn try_from(value: contracts::glue::GoogleProtobufTimestampData) -> Result<Self, Self::Error> {
        Ok(Self {
            seconds: value
                .secs
                .try_into()
                .map_err(TryFromEthAbiTimestampError::Seconds)?,
            nanos: i32::try_from(value.nanos)
                .map_err(TryFromEthAbiTimestampError::NanosTryFromI64)?
                .try_into()
                .map_err(TryFromEthAbiTimestampError::Nanos)?,
        })
    }
}

#[cfg(feature = "ethabi")]
impl crate::EthAbi for Timestamp {
    type EthAbi = contracts::glue::GoogleProtobufTimestampData;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::assert_string_roundtrip;

    macro_rules! ts {
        ($s:literal, $n:literal) => {
            Timestamp {
                seconds: BoundedI64::new($s).unwrap(),
                nanos: BoundedI32::new($n).unwrap(),
            }
        };
    }

    macro_rules! dur {
        ($s:literal, $n:literal) => {
            Duration::new($s, $n).unwrap()
        };
    }

    #[test]
    fn duration_since() {
        assert_eq!(
            ts!(1, 100_000_000).duration_since(&ts!(1, 100_000_000)),
            Some(dur!(0, 000_000_000))
        );
        assert_eq!(
            ts!(2, 100_000_000).duration_since(&ts!(1, 100_000_000)),
            Some(dur!(1, 000_000_000))
        );
        assert_eq!(
            ts!(1, 100_000_000).duration_since(&ts!(2, 100_000_000)),
            Some(dur!(-1, 000_000_000))
        );
        assert_eq!(
            ts!(1, 000_000_000).duration_since(&ts!(2, 100_000_000)),
            Some(dur!(-1, -100_000_000))
        );
        assert_eq!(
            ts!(2, 100_000_000).duration_since(&ts!(1, 000_000_000)),
            Some(dur!(1, 100_000_000))
        );
    }

    #[test]
    fn parse() {
        Timestamp::from_str("2017-01-15T01:30:15.03441Z").unwrap();

        assert_string_roundtrip(&ts!(12345, 6789));
    }

    #[test]
    fn timestamp_duration_arithmetic() {
        // (timestamp.seconds, timestamp.nanos) + (duration) = (timestamp)
        let test_items = [
            // Simple sum
            (
                (100_231_231, 1000),
                (100_000_000, 12),
                Some((200_231_231, 1012)),
            ),
            // Duration contains negative values
            (
                (100_231_111, 2312),
                (-100_000, -12),
                Some((100_131_111, 2300)),
            ),
            // Nanos carry 1 to seconds when the sum > MAX
            (
                (1_234, 100_000_000),
                (1_000, NANOS_MAX - 80_000_000),
                Some((2_235, 19_999_999)),
            ),
            // Nanos carry 1 to seconds when the sum == MAX
            ((1_234, 100_000_000), (1_000, 900_000_000), Some((2_235, 0))),
            // Seconds -1 when nanos < 0
            (
                (1_234, 100_000_000),
                (-1_000, -900_000_000),
                Some((233, 200_000_000)),
            ),
            // None when seconds is not within the bounds
            ((1_234, 0), (TIMESTAMP_SECONDS_MIN - 1_235, 0), None),
            // None when carry from nanos causes seconds to be out of bounds
            (
                (-TIMESTAMP_SECONDS_MIN, 0),
                (2 * TIMESTAMP_SECONDS_MIN, -1),
                None,
            ),
        ];

        for items in test_items {
            assert_eq!(
                Timestamp {
                    seconds: items.0 .0.try_into().unwrap(),
                    nanos: items.0 .1.try_into().unwrap()
                }
                .checked_add(Duration::new(items.1 .0, items.1 .1).unwrap()),
                items.2.map(|(seconds, nanos)| Timestamp {
                    seconds: seconds.try_into().unwrap(),
                    nanos: nanos.try_into().unwrap()
                })
            );
        }
    }
}
