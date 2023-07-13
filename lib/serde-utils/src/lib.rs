extern crate alloc;

use std::fmt::Debug;

use hex::FromHexError;
use serde::de;

const HEX_ENCODING_PREFIX: &str = "0x";

#[derive(Debug)]
pub enum FromHexStringError {
    Hex(FromHexError),
    MissingPrefix(String),
}

impl core::fmt::Display for FromHexStringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FromHexStringError::Hex(e) => write!(f, "{e}"),
            FromHexStringError::MissingPrefix(data) => write!(
                f,
                "missing prefix `{HEX_ENCODING_PREFIX}` when deserializing hex data {data}",
            ),
        }
    }
}

fn to_hex<T: AsRef<[u8]>>(data: T) -> String {
    format!(
        "{HEX_ENCODING_PREFIX}{encoding}",
        encoding = hex::encode(data.as_ref())
    )
}

fn parse_hex<'de, D, T>(string: String) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: TryFrom<Vec<u8>>,
    <T as TryFrom<Vec<u8>>>::Error: Debug,
{
    match string.strip_prefix(HEX_ENCODING_PREFIX) {
        Some(maybe_hex) => hex::decode(maybe_hex).map_err(FromHexStringError::Hex),
        None => Err(FromHexStringError::MissingPrefix(string)),
    }
    .map_err(de::Error::custom)?
    .try_into()
    .map_err(|err| {
        de::Error::custom(format!(
            "type failed to parse bytes from hex data: {err:#?}"
        ))
    })
}

// REVIEW: Are these base64 helpers necessary anymore, since we rarely use the proto types directly?
pub mod base64 {
    use alloc::{string::String, vec::Vec};

    use base64::prelude::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        let encoded = BASE64_STANDARD.encode(bytes);
        String::serialize(&encoded, serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let base64 = String::deserialize(deserializer)?;
        let bytes = BASE64_STANDARD
            .decode(base64.as_bytes())
            .map_err(serde::de::Error::custom)?;

        Ok(bytes)
    }
}

pub mod inner_base64 {
    use alloc::{string::String, vec::Vec};

    use base64::prelude::*;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(
        #[allow(clippy::ptr_arg)] // required by serde
        bytes: &Vec<Vec<u8>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let encoded: Vec<String> = bytes.iter().map(|b| BASE64_STANDARD.encode(b)).collect();
        Vec::serialize(&encoded, serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<Vec<u8>>, D::Error> {
        let vec: Vec<String> = Vec::deserialize(deserializer)?;
        vec.into_iter()
            .map(|item| BASE64_STANDARD.decode(item))
            .collect::<Result<Vec<_>, _>>()
            .map_err(serde::de::Error::custom)
    }
}

// https://play.rust-lang.org/?version=stable&mode=debug&edition=2018&gist=8b514073821e558a5ce862f64361492e
// will optimize this later
pub mod fixed_size_array {
    use std::{convert::TryInto, marker::PhantomData};

    use serde::{
        de::{SeqAccess, Visitor},
        ser::SerializeTuple,
        Deserialize, Deserializer, Serialize, Serializer,
    };

    pub fn serialize<S: Serializer, T: Serialize, const N: usize>(
        data: &[T; N],
        ser: S,
    ) -> Result<S::Ok, S::Error> {
        let mut s = ser.serialize_tuple(N)?;
        for item in data {
            s.serialize_element(item)?;
        }
        s.end()
    }

    struct ArrayVisitor<T, const N: usize>(PhantomData<T>);

    impl<'de, T, const N: usize> Visitor<'de> for ArrayVisitor<T, N>
    where
        T: Deserialize<'de>,
    {
        type Value = [T; N];

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str(&format!("an array of length {}", N))
        }

        #[inline]
        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            // can be optimized using MaybeUninit
            let mut data = Vec::with_capacity(N);
            for _ in 0..N {
                match seq.next_element()? {
                    Some(val) => data.push(val),
                    None => return Err(serde::de::Error::invalid_length(N, &self)),
                }
            }
            match data.try_into() {
                Ok(arr) => Ok(arr),
                Err(_) => unreachable!(),
            }
        }
    }

    pub fn deserialize<'de, D, T, const N: usize>(deserializer: D) -> Result<[T; N], D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        deserializer.deserialize_tuple(N, ArrayVisitor::<T, N>(PhantomData))
    }
}

pub mod hex_string {
    use std::fmt::Debug;

    use serde::Deserialize;

    pub fn serialize<S, T: AsRef<[u8]>>(data: T, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&crate::to_hex(data))
    }

    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
    where
        D: serde::Deserializer<'de>,
        T: TryFrom<Vec<u8>>,
        <T as TryFrom<Vec<u8>>>::Error: Debug,
    {
        String::deserialize(deserializer).and_then(crate::parse_hex::<D, T>)
    }
}

pub mod hex_string_list {
    use alloc::{string::String, vec::Vec};
    use std::fmt::Debug;

    use serde::{de, Deserialize, Deserializer, Serializer};

    pub fn serialize<S, T, C>(list: &C, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: AsRef<[u8]>,
        for<'a> &'a C: IntoIterator<Item = &'a T>,
    {
        serializer.collect_seq(list.into_iter().map(crate::to_hex))
    }

    pub fn deserialize<'de, D, T, C>(deserializer: D) -> Result<C, D::Error>
    where
        D: Deserializer<'de>,
        T: TryFrom<Vec<u8>>,
        <T as TryFrom<Vec<u8>>>::Error: Debug,
        C: TryFrom<Vec<T>>,
        <C as TryFrom<Vec<T>>>::Error: Debug,
    {
        Vec::<String>::deserialize(deserializer)
            .and_then(|x| {
                x.into_iter()
                    .map(crate::parse_hex::<D, T>)
                    .collect::<Result<Vec<_>, _>>()
            })
            .and_then(|vec| {
                vec.try_into()
                    .map_err(|err| de::Error::custom(format!("failed to collect list: {err:#?}")))
            })
    }
}

pub mod string {
    use std::{fmt, str::FromStr};

    use serde::de::Deserialize;

    pub fn serialize<S, T>(data: T, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
        T: fmt::Display,
    {
        serializer.collect_str(&data)
    }

    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
    where
        D: serde::Deserializer<'de>,
        T: FromStr,
    {
        let s: String = <String>::deserialize(deserializer)?;
        let inner: T = s
            .parse()
            // TODO fix error situation
            // FromStr::Err has no bounds
            .map_err(|_| serde::de::Error::custom("failure to parse string data"))?;
        Ok(inner)
    }
}
